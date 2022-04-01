use lazy_static::lazy_static;
use prometheus::{register_histogram, Histogram};

lazy_static! {
    // provides the events latency + the total events count
    pub static ref DELTA_T: Histogram =
        register_histogram!(
        "event_delta",
        "Time difference (in seconds) between ingress and processing"
    )
    .unwrap();
}
