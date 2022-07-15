use super::{CLIENT_TIMEOUT, HEARTBEAT_INTERVAL};
use crate::notifier::{Request, Response};
use actix::{Actor, ActorContext, AsyncContext, Handler, SpawnHandle, StreamHandler, WrapFuture};
use actix_web_actors::ws::{self, CloseCode, CloseReason};
use drogue_doppelgaenger_core::{
    listener::{KafkaSource, Message},
    notifier::Notifier,
    service::{Id, Service},
    storage::Storage,
};
use futures::StreamExt;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

mod message {
    use crate::notifier::Response;
    use actix::Message;
    use actix_web_actors::ws::CloseReason;

    #[derive(Message)]
    #[rtype(result = "()")]
    pub struct Subscribe(pub String);
    #[derive(Message)]
    #[rtype(result = "()")]
    pub struct Unsubscribe(pub String);
    #[derive(Message)]
    #[rtype(result = "Result<(), serde_json::Error>")]
    pub struct Event(pub Response);
    #[derive(Message)]
    #[rtype(result = "()")]
    pub struct Close(pub Option<CloseReason>);
}

pub struct WebSocketHandler<S: Storage, N: Notifier> {
    heartbeat: Instant,
    listeners: HashMap<Id, SpawnHandle>,
    service: Arc<Service<S, N>>,
    source: Arc<KafkaSource>,
    application: String,
    /// Whether or not to just subscribe for a single thing
    thing: Option<String>,
}

impl<S: Storage, N: Notifier> WebSocketHandler<S, N> {
    pub fn new(
        service: Arc<Service<S, N>>,
        source: Arc<KafkaSource>,
        application: String,
        thing: Option<String>,
    ) -> Self {
        Self {
            heartbeat: Instant::now(),
            listeners: Default::default(),
            service,
            source,
            application,
            thing,
        }
    }

    fn start_heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > CLIENT_TIMEOUT {
                log::warn!("Disconnecting failed heartbeat");
                ctx.stop();
                return;
            }

            ctx.ping(b"PING");
        });
    }

    /// Handle the parse result of a client protocol message.
    fn handle_protocol_message(
        &self,
        ctx: &mut ws::WebsocketContext<Self>,
        result: Result<Request, serde_json::Error>,
    ) {
        match result {
            Ok(Request::Subscribe { thing }) if self.thing.is_none() => {
                ctx.address().do_send(message::Subscribe(thing));
            }
            Ok(Request::Unsubscribe { thing }) if self.thing.is_none() => {
                ctx.address().do_send(message::Unsubscribe(thing));
            }
            Ok(Request::Subscribe { .. } | Request::Unsubscribe { .. }) => {
                ctx.close(Some(CloseReason {
                    code: CloseCode::Unsupported,
                    description: Some(format!(
                        "Unable to subscribe/unsubscribe on single-thing endpoint"
                    )),
                }));
                ctx.stop();
            }
            Err(err) => {
                ctx.close(Some(CloseReason {
                    code: CloseCode::Protocol,
                    description: Some(err.to_string()),
                }));
                ctx.stop();
            }
        }
    }
}

impl<S: Storage, N: Notifier> Actor for WebSocketHandler<S, N> {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        log::info!("Starting WS handler");
        self.start_heartbeat(ctx);
        if let Some(thing) = &self.thing {
            log::info!("Starting in single-thing mode: {thing}");
            if let Err(err) = ctx.address().try_send(message::Subscribe(thing.clone())) {
                log::warn!("Failed to initialize single-thing listener: {err}");
                ctx.close(Some(CloseReason {
                    code: CloseCode::Abnormal,
                    description: Some("Failed to establish initial subscription".to_string()),
                }));
                ctx.stop();
            }
        }
    }
}

// Handle incoming messages from the Websocket Client
impl<S: Storage, N: Notifier> StreamHandler<Result<ws::Message, ws::ProtocolError>>
    for WebSocketHandler<S, N>
{
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.heartbeat = Instant::now();
            }
            Ok(ws::Message::Binary(data)) => {
                self.handle_protocol_message(ctx, serde_json::from_slice::<super::Request>(&data));
            }
            Ok(ws::Message::Text(data)) => {
                self.handle_protocol_message(ctx, serde_json::from_str::<super::Request>(&data));
            }
            Ok(ws::Message::Close(reason)) => {
                log::debug!("Client disconnected - reason: {:?}", reason);
                ctx.close(reason);
                ctx.stop();
            }
            Ok(ws::Message::Continuation(_)) => {
                ctx.stop();
            }
            Ok(ws::Message::Nop) => {}
            Err(e) => {
                log::error!("WebSocket Protocol Error: {}", e);
                ctx.stop()
            }
        }
    }
}

impl<S: Storage, N: Notifier> Handler<message::Subscribe> for WebSocketHandler<S, N> {
    type Result = ();

    fn handle(&mut self, msg: message::Subscribe, ctx: &mut Self::Context) -> Self::Result {
        let id = Id {
            application: self.application.clone(),
            thing: msg.0,
        };
        if self.listeners.contains_key(&id) {
            return;
        }

        let service = self.service.clone();

        let mut source = self.source.subscribe(id.clone());
        let addr = ctx.address();
        let i = id.clone();
        let task = ctx.spawn(
            async move {
                // read the initial state
                // FIXME: filter out "not found"
                if let Ok(thing) = service.get(id).await {
                    let initial_generation = thing.metadata.generation;
                    // send initial
                    addr.do_send(message::Event(Response::Change {
                        thing: Arc::new(thing),
                    }));

                    while let Some(msg) = source.next().await {
                        match msg {
                            Ok(Message::Change(thing)) => {
                                if thing.metadata.generation > initial_generation {
                                    // prevent initial duplicates
                                    addr.do_send(message::Event(Response::Change { thing }))
                                } else {
                                    log::info!("Suppressing duplicate generation change");
                                }
                            }
                            Err(BroadcastStreamRecvError::Lagged(lag)) => {
                                addr.do_send(message::Event(Response::Lag { lag }))
                            }
                        }
                    }
                    log::warn!("Listener loop exited");
                }

                // stream closed
                addr.do_send(message::Close(Some(CloseReason {
                    code: CloseCode::Error,
                    description: Some("Notifier stream closed".to_string()),
                })));
            }
            .into_actor(self),
        );

        self.listeners.insert(i, task);
    }
}

impl<S: Storage, N: Notifier> Handler<message::Unsubscribe> for WebSocketHandler<S, N> {
    type Result = ();

    fn handle(&mut self, msg: message::Unsubscribe, ctx: &mut Self::Context) -> Self::Result {
        let id = Id {
            application: self.application.clone(),
            thing: msg.0,
        };
        if let Some(task) = self.listeners.remove(&id) {
            ctx.cancel_future(task);
        }
    }
}

impl<S: Storage, N: Notifier> Handler<message::Event> for WebSocketHandler<S, N> {
    type Result = Result<(), serde_json::Error>;

    fn handle(&mut self, msg: message::Event, ctx: &mut Self::Context) -> Self::Result {
        Ok(ctx.text(serde_json::to_string(&msg.0)?))
    }
}

impl<S: Storage, N: Notifier> Handler<message::Close> for WebSocketHandler<S, N> {
    type Result = ();

    fn handle(&mut self, msg: message::Close, ctx: &mut Self::Context) -> Self::Result {
        ctx.close(msg.0);
        ctx.stop();
    }
}
