use super::{CLIENT_TIMEOUT, HEARTBEAT_INTERVAL};
use crate::notifier::{Request, Response, SetDesiredValue};
use actix::{
    Actor, ActorContext, AsyncContext, Handler, ResponseFuture, SpawnHandle, StreamHandler,
    WrapFuture,
};
use actix_web_actors::ws::{self, CloseCode, CloseReason};
use drogue_doppelgaenger_core::command::CommandSink;
use drogue_doppelgaenger_core::{
    listener::{KafkaSource, Message},
    notifier::Notifier,
    processor::{self, sink::Sink, Event},
    service::{Id, Service},
    storage::Storage,
};
use futures::StreamExt;
use std::{collections::BTreeMap, collections::HashMap, fmt::Display, sync::Arc, time::Instant};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

mod message {
    use crate::notifier::Response;
    use actix::Message;
    use actix_web_actors::ws::CloseReason;
    use drogue_doppelgaenger_core::processor::SetDesiredValue;
    use std::collections::BTreeMap;

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
    pub struct SetDesiredValues(pub String, pub BTreeMap<String, SetDesiredValue>);
    #[derive(Message)]
    #[rtype(result = "()")]
    pub struct Close(pub Option<CloseReason>);
}

pub struct WebSocketHandler<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> {
    heartbeat: Instant,
    listeners: HashMap<Id, SpawnHandle>,
    service: Arc<Service<S, N, Si, Cmd>>,
    source: Arc<KafkaSource>,
    application: String,
    /// Whether or not to just subscribe for a single thing
    thing: Option<String>,
}

impl<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> WebSocketHandler<S, N, Si, Cmd> {
    pub fn new(
        service: Arc<Service<S, N, Si, Cmd>>,
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
                    description: Some(
                        "Unable to subscribe/unsubscribe on single-thing endpoint".to_string(),
                    ),
                }));
                ctx.stop();
            }
            Ok(Request::SetDesiredValues { thing, values }) => match Self::convert_set(values) {
                Ok(values) => {
                    ctx.address()
                        .do_send(message::SetDesiredValues(thing, values));
                }
                Err(err) => {
                    Self::close_err(ctx, err);
                }
            },
            Err(err) => {
                Self::close_err(ctx, err);
            }
        }
    }

    fn convert_set(
        value: BTreeMap<String, SetDesiredValue>,
    ) -> anyhow::Result<BTreeMap<String, processor::SetDesiredValue>> {
        value
            .into_iter()
            .map(|(key, value)| {
                let value: processor::SetDesiredValue = value.try_into()?;
                Ok((key, value))
            })
            .collect::<Result<BTreeMap<String, processor::SetDesiredValue>, _>>()
    }

    fn close_err<E: Display>(ctx: &mut ws::WebsocketContext<Self>, err: E) {
        log::info!("Closing websocket due to: {err}");
        ctx.close(Some(CloseReason {
            code: CloseCode::Protocol,
            description: Some(err.to_string()),
        }));
        ctx.stop();
    }
}

impl<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> Actor
    for WebSocketHandler<S, N, Si, Cmd>
{
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
impl<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>
    StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebSocketHandler<S, N, Si, Cmd>
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
                self.handle_protocol_message(ctx, serde_json::from_slice(&data));
            }
            Ok(ws::Message::Text(data)) => {
                log::debug!("Message: {data}");
                self.handle_protocol_message(ctx, serde_json::from_str(&data));
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

impl<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> Handler<message::Subscribe>
    for WebSocketHandler<S, N, Si, Cmd>
{
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

        // subscribe first
        let mut source = self.source.subscribe(id.clone());

        let addr = ctx.address();
        let i = id.clone();
        let task = ctx.spawn(
            async move {
                // now read the initial state
                let initial_generation = match service.get(&id).await {
                    Ok(Some(thing)) => {
                        let initial_generation = thing.metadata.generation;
                        // send initial
                        addr.do_send(message::Event(Response::Initial {
                            thing: Arc::new(thing),
                        }));
                        initial_generation
                    }
                    Ok(None) => Some(0),
                    Err(err) => {
                        // stream closed
                        addr.do_send(message::Close(Some(CloseReason {
                            code: CloseCode::Abnormal,
                            description: Some("Failed to read initial state".to_string()),
                        })));

                        log::warn!("Failed to read initial state: {err}");
                        return;
                    }
                };

                // and run the loop
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

impl<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> Handler<message::Unsubscribe>
    for WebSocketHandler<S, N, Si, Cmd>
{
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

impl<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> Handler<message::SetDesiredValues>
    for WebSocketHandler<S, N, Si, Cmd>
{
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: message::SetDesiredValues, _ctx: &mut Self::Context) -> Self::Result {
        let application = self.application.clone();
        let sink = self.service.sink().clone();

        Box::pin(async move {
            let thing = msg.0;
            let message = processor::Message::SetDesiredValue { values: msg.1 };

            let _ = sink.publish(Event::new(application, thing, message)).await;
        })
    }
}

impl<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> Handler<message::Event>
    for WebSocketHandler<S, N, Si, Cmd>
{
    type Result = Result<(), serde_json::Error>;

    fn handle(&mut self, msg: message::Event, ctx: &mut Self::Context) -> Self::Result {
        ctx.text(serde_json::to_string(&msg.0)?);
        Ok(())
    }
}

impl<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> Handler<message::Close>
    for WebSocketHandler<S, N, Si, Cmd>
{
    type Result = ();

    fn handle(&mut self, msg: message::Close, ctx: &mut Self::Context) -> Self::Result {
        ctx.close(msg.0);
        ctx.stop();
    }
}
