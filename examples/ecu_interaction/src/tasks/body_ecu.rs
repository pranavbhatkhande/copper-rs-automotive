//! Body ECU Simulation Task.
//!
//! This task simulates a Body Control Module (BCM) that:
//! 1. Periodically toggles a turn signal CAN message (ID 0x456).
//! 2. Listens for speed information from the Meter (ID 0x123).

use cu_automotive_payloads::can::{CanFrame, CanId, CanFlags};
use cu29::prelude::*;

#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct BodyEcu {
    turn_signal_active: bool,
    last_signal_time: CuTime,
    received_speed: f32,
}

impl Freezable for BodyEcu {}

impl CuTask for BodyEcu {
    type Resources<'r> = ();
    type Input<'m> = input_msg!(CanFrame);
    type Output<'m> = output_msg!(CanFrame);

    fn new(config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self> {
        Ok(Self {
            turn_signal_active: false,
            last_signal_time: CuTime::default(),
            received_speed: 0.0,
        })
    }

    fn process<'i, 'o>(
        &mut self,
        ctx: &CuContext,
        input: &Self::Input<'i>,
        output: &mut Self::Output<'o>,
    ) -> CuResult<()> {
        let now = ctx.now();

        // 1. Handle incoming messages from Meter (Speed)
        if let Some(frame) = input.payload() {
            if let CanId::Standard(0x123) = frame.id {
                if frame.dlc >= 4 {
                    let mut speed_bytes = [0u8; 4];
                    speed_bytes.copy_from_slice(&frame.data[0..4]);
                    self.received_speed = f32::from_be_bytes(speed_bytes);
                }
            }
        }

        // 2. Generate Turn Signal message every 2 seconds
        if now.saturating_sub(self.last_signal_time) > CuDuration::from_secs(2).into() {
            self.turn_signal_active = !self.turn_signal_active;
            self.last_signal_time = now;

            let mut data = [0u8; 8];
            data[0] = if self.turn_signal_active { 1 } else { 0 };
            
            let frame = CanFrame {
                id: CanId::Standard(0x456),
                dlc: 1,
                flags: CanFlags::NONE,
                data,
            };
            output.set_payload(frame);
            output.tov = Tov::Time(now);
        }

        Ok(())
    }
}
