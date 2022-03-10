use crate::processor::TwinEvent;
use lazy_static::lazy_static;
use prometheus::{register_histogram, register_int_counter, Histogram, IntCounter};
use std::collections::HashMap;

lazy_static! {
    // provides the events latency + the total events count
    pub static ref DELTA_T: Histogram =
        register_histogram!(
        "burrboard_event_delta",
        "Time difference (in seconds) between ingress and processing"
    )
    .unwrap();
    pub static ref DEVICES_SEEN: IntCounter =
        register_int_counter!("total_burrboard_devices", "the number of uniques devices that sent data to drogue-cloud").unwrap();
    pub static ref BUTTON_A_PRESSES: IntCounter =
        register_int_counter!("burrboard_buttons_a_presses", "the number of total button A presses").unwrap();
    pub static ref BUTTON_B_PRESSES: IntCounter =
        register_int_counter!("burrboard_buttons_b_presses", "the number of total button B presses").unwrap();
    pub static ref TEMPERATURE_VALUES: Histogram =
        register_histogram!("burrboard_temperature", "the temperature reported by a burrboard").unwrap();
    pub static ref LIGHT_VALUES: Histogram =
        register_histogram!("burrboard_light", "the light reported by a burrboard").unwrap();
}

pub struct Metrics {
    devices: HashMap<String, Buttons>,
}

impl Metrics {
    pub fn new() -> Self {
        Metrics {
            devices: HashMap::new(),
        }
    }

    pub fn process(&mut self, event: &TwinEvent) {
        let device = &event.device;

        // extract sensors values
        let button_a = event.features.get("button_a");
        let button_b = event.features.get("button_b");
        let light = event.features.get("light");
        let temp = event.features.get("temperature");

        // if the payload don't contains those fields it's probably not from a burrboard so we do nothing with it
        if let (Some(a), Some(b), Some(light), Some(temp)) = (button_a, button_b, light, temp) {
            let a = a
                .get("presses")
                .map(|v| v.as_u64().unwrap_or_default())
                .unwrap();
            let b = b
                .get("presses")
                .map(|v| v.as_u64().unwrap_or_default())
                .unwrap();
            let light = light
                .get("value")
                .map(|v| v.as_f64().unwrap_or_default())
                .unwrap();
            let temp = temp
                .get("value")
                .map(|v| v.as_f64().unwrap_or_default())
                .unwrap();

            // we don't need to cache temp and light data
            TEMPERATURE_VALUES.observe(temp);
            LIGHT_VALUES.observe(light);

            // update button presses devices
            if let Some(buttons) = self.devices.get_mut(device) {
                buttons.update(a, b);
            }
            // this is the first time we receive a message from the device.
            else {
                self.devices.insert(device.clone(), Buttons::new(a, b));
                BUTTON_A_PRESSES.inc_by(a);
                BUTTON_B_PRESSES.inc_by(b);
                DEVICES_SEEN.inc();
            }
        }
    }
}

pub struct Buttons {
    button_a_total: u64,
    button_b_total: u64,
}

impl Buttons {
    fn new(a: u64, b: u64) -> Self {
        Buttons {
            button_a_total: a,
            button_b_total: b,
        }
    }

    fn update(&mut self, a: u64, b: u64) {
        let new_a_presses = a.checked_sub(self.button_a_total).unwrap_or_default();
        let new_b_presses = b.checked_sub(self.button_b_total).unwrap_or_default();

        BUTTON_A_PRESSES.inc_by(new_a_presses);
        BUTTON_B_PRESSES.inc_by(new_b_presses);

        self.button_a_total = a;
        self.button_b_total = b;
    }
}
