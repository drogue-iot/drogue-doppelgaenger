import json
import os
import signal
import sys

import tornado.websocket
import tornado.httpserver
import tornado.ioloop
import tornado.web
from motor import MotorClient
from dotenv import load_dotenv
from logzero import logger
from bson import json_util
from healthcheck import TornadoHandler, HealthCheck
import time

health = HealthCheck()


class ChangesHandler(tornado.websocket.WebSocketHandler):
    connected_clients = set()
    collection = None

    def initialize(self):
        self.collection = self.application.settings.get('collection')
        logger.info(f"Collection: {self.collection}")

    def check_origin(self, origin):
        return True

    async def send_current(self):
        async for document in self.collection.find({}):
            message = {
                "operation": "init",
                "document": document,
            }
            await self.write_message(json.dumps(message, default=json_util.default))

    async def open(self):
        ChangesHandler.connected_clients.add(self)
        await self.send_current()

    def on_close(self):
        ChangesHandler.connected_clients.remove(self)

    @classmethod
    def send_updates(cls, message):
        logger.debug(f"sending update: {message} to {len(cls.connected_clients)} clients")
        for connected_client in cls.connected_clients:
            connected_client.write_message(message)

    @classmethod
    def on_change(cls, change):
        message = {
            "operation": change['operationType'],
            "document": change['fullDocument'],
        }
        ChangesHandler.send_updates(json.dumps(message, default=json_util.default))


change_stream = None


async def watch(collection):
    global change_stream

    async with collection.watch(full_document="updateLookup") as change_stream:
        async for change in change_stream:
            ChangesHandler.on_change(change)


class HomeHandler(tornado.web.RequestHandler):
    def get(self):
        self.render("templates/index.html",
                    simulator_url=os.getenv("SIMULATOR_URL", ""),
                    title=os.getenv("INDEX__TITLE", "Dashboard"),
                    cols=os.getenv("INDEX__COLS", "row-cols-1 row-cols-md-2 row-cols-xl-4 g-4")
                    )


def sig_handler(sig, frame):
    logger.warning('Caught signal: %s', sig)
    os._exit(0)


def main():
    load_dotenv()

    database = os.environ["DATABASE"]
    application = os.environ["DROGUE_APP"]

    logger.info('Database: %s, collection: %s', database, application)

    client = MotorClient(os.environ["MONGODB__URL"])
    collection = client[database][application]

    global app
    global loop

    app = tornado.web.Application(
        [
            (r"/socket", ChangesHandler),
            (
                "/health",
                TornadoHandler, dict(checker=health)
            ),
            (
                r"/", HomeHandler
            ),
            (
                r"/(.*)", tornado.web.StaticFileHandler,
                {"path": "templates/", "default_filename": "index.html"}
            )
        ],
        collection=collection
    )

    app.listen(8082)

    signal.signal(signal.SIGTERM, sig_handler)
    signal.signal(signal.SIGINT, sig_handler)

    loop = tornado.ioloop.IOLoop.current()
    loop.add_callback(watch, collection)

    try:
        loop.start()
    except KeyboardInterrupt:
        pass
    finally:
        if change_stream is not None:
            change_stream.close()

    logger.warning("Exiting...")


if __name__ == "__main__":
    main()
