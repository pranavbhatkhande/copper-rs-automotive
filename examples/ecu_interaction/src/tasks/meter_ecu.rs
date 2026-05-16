//! Meter ECU Simulation Task (Sink).
//!
//! This task simulates an Instrument Cluster (Meter) that:
//! 1. Listens for turn signal status from the Body ECU (ID 0x456).
//! 2. Logs the turn signal state (speed response is simulated internally).

use cu_automotive_payloads::can::{CanFrame, CanId};
use cu29::prelude::*;

#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct MeterEcu {
    turn_signal_received: bool,
    last_signal_time: CuTime,
}

impl Freezable for MeterEcu {}

impl CuSinkTask for MeterEcu {
    type Resources<'r> = ();
    type Input<'m> = input_msg!(CanFrame);

    fn new(_config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self> {
        Ok(Self {
            turn_signal_received: false,
            last_signal_time: CuTime::default(),
        })
    }

    fn process<'i>(&mut self, _ctx: &CuContext, input: &Self::Input<'i>) -> CuResult<()> {
        let now = _ctx.now();

        // Handle incoming messages from Body (Turn Signal)
        if let Some(frame) = input.payload() {
            if let CanId::Standard(0x456) = frame.id {
                self.turn_signal_received = frame.data[0] != 0;
                self.last_signal_time = now;
                info!(
                    "MeterEcu: Turn signal {} (speed: 60 km/h simulated)",
                    if self.turn_signal_received {
                        "ON"
                    } else {
                        "OFF"
                    }
                );
            }
        }

        Ok(())
    }

    fn stop(&mut self, _ctx: &CuContext) -> CuResult<()> {
        Ok(())
    }
}
