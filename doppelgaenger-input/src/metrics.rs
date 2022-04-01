use crate::processor::TwinEvent;
use lazy_static::lazy_static;
use prometheus::{register_histogram, register_int_counter, Histogram, IntCounter};
use std::collections::HashMap;

lazy_static! {
    // provides the events latency + the total events count
    pub static ref DELTA_T: Histogram =
        register_histogram!(
        "event_delta",
        "Time difference (in seconds) between ingress and processing"
    )
    .unwrap();
}
