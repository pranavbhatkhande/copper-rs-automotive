//! Meter ECU Simulation Task.
//!
//! This task simulates an Instrument Cluster (Meter) that:
//! 1. Listens for turn signal status from the Body ECU (ID 0x456).
//! 2. If the turn signal is active, it sends a speed message (ID 0x123) back to the Body.

use cu_automotive_payloads::can::{CanFrame, CanId, CanFlags};
use cu29::prelude::*;

#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct MeterEcu {
    turn_signal_received: bool,
    last_signal_time: CuTime,
}

impl Freezable for MeterEcu {}

impl CuTask for MeterEcu {
    type Resources<'r> = ();
    type Input<'m> = input_msg!(CanFrame);
    type Output<'m> = output_msg!(CanFrame);

    fn new(config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self> {
        Ok(Self {
            turn_signal_received: false,
            last_signal_time: CuTime::default(),
        })
    }

    fn process<'i, 'o>(
        &mut self,
        ctx: &CuContext,
        input: &Self::Input<'i>,
        output: &mut Self::Output<'o>,
    ) -> CuResult<()> {
        let now = ctx.now();

        // 1. Handle incoming messages from Body (Turn Signal)
        if let Some(frame) = input.payload() {
            if let CanId::Standard(0x456) = frame.id {
                self.turn_signal_received = frame.data[0] != 0;
                self.last_signal_time = now;
            }
        }

        // 2. If turn signal was active, send speed message
        // We simulate that the meter only sends speed if it knows the turn signal is active.
        if self.turn_signal_received && now.saturating_sub(self.last_signal_time) < CuDuration::from_secs(5).into() {
            let mut data = [0u8; 8];
            let speed: f32 = 60.0; // Constant speed for simulation
            let speed_bytes = speed.to_be_bytes();
            data[0..4].copy_from_slice(&speed_bytes);

            let frame = CanFrame {
                id: CanId::Standard(0x123),
                dlc: 4,
                flags: CanFlags::NONE,
                data,
            };
            output.set_payload(frame);
            output.tov = Tov::Time(now);
        }

        Ok(())
    }
}
