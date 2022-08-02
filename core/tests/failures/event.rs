use crate::common::mock::{Builder, MockCommandSink, RunningContext};
use anyhow::anyhow;
use indexmap::IndexMap;
use serde_json::json;
use std::future::Future;

use crate::common::failure::{Failure, Iter};
use crate::common::mock::{MockNotifier, MockSink, MockStorage};
use drogue_doppelgaenger_core::model::{Changed, Code, Reconciliation, Thing};
use drogue_doppelgaenger_core::processor::{Event, Message, ReportStateBuilder};
use drogue_doppelgaenger_core::service::{Error, Id, Service, UpdateOptions, POSTPONE_DURATION};

pub struct TestRunner<'t> {
    source: &'t Id,
    target: &'t Id,
    opts: &'t UpdateOptions,
    service: &'t Service<MockStorage, MockNotifier, MockSink, MockCommandSink>,
    notifier: &'t mut MockNotifier,
    sink: &'t mut MockSink,
}

impl<'t> TestRunner<'t> {
    pub async fn step(&mut self, expected: Result<(usize, Vec<usize>), String>) {
        let actual = self
            .service
            .update(
                self.source,
                ReportStateBuilder::partial().state("foo", "bar"),
                self.opts,
            )
            .await;

        self.assert_step(actual, expected).await;
    }

    async fn assert_step(
        &mut self,
        actual: Result<Thing, Error<MockStorage, MockNotifier, MockCommandSink>>,
        expected: Result<(usize, Vec<usize>), String>,
    ) {
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => {
                // compare
                assert_eq!(actual.reported_state["counter"].value, json!(expected.0));
                assert_eq!(self.notifier.drain().await, vec![actual.clone()]);

                let events = self.sink.drain().await;
                assert_events(
                    events,
                    MockEvent::iter(
                        self.target,
                        expected
                            .1
                            .into_iter()
                            .map(|counter| Message::Merge(json!({ "counter": counter }))),
                    ),
                );
            }
            (Err(err), Err(message)) => {
                // ok, but check message
                assert_eq!(err.to_string(), message);
            }
            (left, right) => {
                panic!(
                    r#"assertion failed: `(left == right)`
  left: `{:?}`,
 right: `{:?}`"#,
                    left, right,
                )
            }
        }
    }
}

pub trait TestFn<'a, I: 'a, O> {
    type Output: Future<Output = O> + 'a;
    fn call(self, input: I) -> Self::Output;
}

impl<'a, I: 'a, O, F, Fut> TestFn<'a, I, O> for F
where
    F: FnOnce(I) -> Fut,
    Fut: Future<Output = O> + 'a,
{
    type Output = Fut;

    fn call(self, input: I) -> Self::Output {
        self(input)
    }
}

async fn run_test_2<F>(
    failures: Failure<(), anyhow::Error>,
    opts: UpdateOptions,
    initial: Result<(usize, Vec<usize>), String>,
    f: F,
) -> anyhow::Result<()>
where
    for<'a> F: TestFn<'a, &'a mut TestRunner<'a>, Result<(), anyhow::Error>>,
{
    let RunningContext {
        service,
        mut notifier,
        runner,
        mut sink,
        ..
    } = Builder::new().sink_failure(failures).setup().run(false);

    let id = Id::new("default", "thing1");
    let id2 = Id::new("default", "thing2");

    // first one is ok
    let result = service
        .create(Thing {
            reconciliation: Reconciliation {
                changed: {
                    let mut map = IndexMap::new();
                    map.insert(
                        "test".to_string(),
                        Changed::from(Code::JavaScript(
                            r#"
// send empty strategic merge, every time we reconcile

if (context.newState.reportedState === undefined) {
    context.newState.reportedState = {}
}
const value = (context.newState.reportedState["counter"]?.value || 0) + 1; 
context.newState.reportedState["counter"] = { value, lastUpdate: new Date().toISOString() };

context.outbox.push({thing: "thing2", message: { merge: {"counter": value}}});
"#
                            .to_string(),
                        )),
                    );
                    map
                },
                ..Default::default()
            },
            ..Thing::with_id(&id)
        })
        .await;

    let mut tester = TestRunner {
        source: &id,
        target: &id2,
        opts: &opts,
        service: &service,
        notifier: &mut notifier,
        sink: &mut sink,
    };

    // initial
    tester.assert_step(result, initial).await;

    // run
    f.call(&mut tester).await.unwrap();

    // done
    runner.shutdown().await
}

#[tokio::test]
async fn test_resend_event() {
    run_test_2(
        failures([false, true]),
        UpdateOptions {
            ignore_unclean_inbox: false,
        },
        Ok((1, vec![1])),
        {
            async fn test(test: &mut TestRunner<'_>) -> anyhow::Result<()> {
                test.step(Ok((2, vec![]))).await;
                test.step(Ok((3, vec![2, 3]))).await;

                Ok(())
            }

            test
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn test_resend_event_2() {
    run_test_2(
        failures([false, true, true, false]),
        UpdateOptions {
            ignore_unclean_inbox: false,
        },
        Ok((1, vec![1])),
        {
            async fn test(test: &mut TestRunner<'_>) -> anyhow::Result<()> {
                // fail 1
                test.step(Ok((2, vec![]))).await;
                // fail 2 (when retrying)
                test.step(Err("Unclean Outbox".to_string())).await;
                // fails because events are delayed now
                test.step(Err("Unclean Outbox".to_string())).await;
                // now we can retry (and consumed all failures)
                tokio::time::sleep(POSTPONE_DURATION).await;
                // so all events should be there, and we should succeed
                test.step(Ok((3, vec![2, 3]))).await;

                Ok(())
            }

            test
        },
    )
    .await
    .unwrap();
}

fn failures<F>(failures: F) -> Failure<(), anyhow::Error>
where
    F: IntoIterator<Item = bool> + Send + Sync,
    <F as IntoIterator>::IntoIter: Send + 'static,
{
    Failure::new(Iter(failures.into_iter().enumerate().map(
        |(i, f)| match f {
            true => Some(anyhow!("Expected #{i}")),
            false => None,
        },
    )))
}

fn assert_events<I1, I2>(left: I1, right: I2)
where
    I1: IntoIterator<Item = Event>,
    I2: IntoIterator<Item = MockEvent>,
{
    let left = left.into_iter().map(Into::into).collect::<Vec<MockEvent>>();
    let right = right.into_iter().collect::<Vec<_>>();

    assert_eq!(left, right);
}

#[derive(Clone, Debug, PartialEq)]
pub struct MockEvent {
    pub application: String,
    pub device: String,
    pub message: Message,
}

impl From<Event> for MockEvent {
    fn from(event: Event) -> Self {
        Self {
            application: event.application,
            device: event.device,
            message: event.message,
        }
    }
}

impl MockEvent {
    pub fn new(id: &Id, message: Message) -> MockEvent {
        Self {
            application: id.application.clone(),
            device: id.thing.clone(),
            message,
        }
    }

    pub fn iter<I>(id: &Id, messages: I) -> impl Iterator<Item = MockEvent>
    where
        I: IntoIterator<Item = Message>,
    {
        let id = id.clone();
        messages.into_iter().map(move |m| MockEvent::new(&id, m))
    }
}
