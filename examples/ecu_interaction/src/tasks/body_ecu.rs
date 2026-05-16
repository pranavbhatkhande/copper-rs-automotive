//! Body ECU Simulation Task (Source).
//!
//! This task simulates a Body Control Module (BCM) that:
//! 1. Periodically toggles a turn signal CAN message (ID 0x456).

use cu_automotive_payloads::can::{CanFlags, CanFrame, CanId};
use cu29::prelude::*;

#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct BodyEcu {
    turn_signal_active: bool,
    last_signal_time: CuTime,
}

impl Freezable for BodyEcu {}

impl CuSrcTask for BodyEcu {
    type Resources<'r> = ();
    type Output<'m> = output_msg!(CanFrame);

    fn new(_config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self> {
        Ok(Self {
            turn_signal_active: false,
            last_signal_time: CuTime::default(),
        })
    }

    fn preprocess(&mut self, _ctx: &CuContext) -> CuResult<()> {
        Ok(())
    }

    fn process<'o>(&mut self, ctx: &CuContext, output: &mut Self::Output<'o>) -> CuResult<()> {
        let now = ctx.now();

        // Generate Turn Signal message every 2 seconds
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

    fn stop(&mut self, _ctx: &CuContext) -> CuResult<()> {
        Ok(())
    }
}
