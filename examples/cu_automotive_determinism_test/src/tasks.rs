//! Automotive determinism test tasks.
//!
//! AutoTestSource: Generates a deterministic sequence of UDS requests
//! that exercises session management, security access, DID operations,
//! and deliberately includes time gaps to trigger S3 timeout.
//!
//! AutoTestSink: Captures responses (no-op in sim mode).

use cu_automotive_payloads::isotp::IsotpPdu;
use cu29::prelude::*;

/// Deterministic sequence of UDS request frames.
/// Each entry is (iteration_number, raw_bytes).
/// Gaps between iterations trigger timer-dependent behavior.
pub const TEST_SEQUENCE: &[(usize, &[u8])] = &[
    // Iteration 0: DiagnosticSessionControl → Extended (0x03)
    (0, &[0x10, 0x03]),
    // Iteration 1: TesterPresent
    (1, &[0x3E, 0x00]),
    // Iteration 2: ReadDID VIN (0xF190)
    (2, &[0x22, 0xF1, 0x90]),
    // Iteration 3: SecurityAccess requestSeed (level 1)
    (3, &[0x27, 0x01]),
    // Iteration 4: SecurityAccess sendKey (level 2, key = seed XOR 0xDEAD)
    // The actual key depends on the seed, so we'll handle this dynamically
    (4, &[0x27, 0x02, 0x00, 0x00]),
    // Iteration 5: WriteDID 0xF200 = [0xAA, 0xBB]
    (5, &[0x2E, 0xF2, 0x00, 0xAA, 0xBB]),
    // Iteration 6: ReadDID 0xF200 (read back what we wrote)
    (6, &[0x22, 0xF2, 0x00]),
    // Iteration 7: TesterPresent (keep-alive)
    (7, &[0x3E, 0x00]),
    // Iteration 10: EcuReset soft (0x03)
    (10, &[0x11, 0x03]),
    // Iteration 11: DiagnosticSessionControl → Default (0x01)
    (11, &[0x10, 0x01]),
    // Iteration 12: ReadDID VIN again (should work in Default)
    (12, &[0x22, 0xF1, 0x90]),
    // Iteration 15: DiagnosticSessionControl → Extended again
    (15, &[0x10, 0x03]),
    // Then a deliberate gap — no requests from 16 to 30
    // At iteration 30 (with enough time delta), S3 timeout should fire
    // Iteration 30: TesterPresent (should be back in Default session due to S3 timeout)
    (30, &[0x3E, 0x00]),
    // Iteration 31: DiagnosticSessionControl → Extended (re-enter after timeout)
    (31, &[0x10, 0x03]),
    // Iteration 32: ReadDID VIN
    (32, &[0x22, 0xF1, 0x90]),
    // Iteration 40: RoutineControl start (0x01) for routine 0xFF00
    (40, &[0x31, 0x01, 0xFF, 0x00]),
    // Iteration 45: ClearDTC
    (45, &[0x14, 0xFF, 0xFF, 0xFF]),
    // Iteration 50: Unknown SID (should get ServiceNotSupported NRC)
    (50, &[0xBB]),
];

/// Source task that generates deterministic UDS request PDUs.
#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct AutoTestSource {
    iteration: usize,
    seq_idx: usize,
}

impl Freezable for AutoTestSource {}

impl CuSrcTask for AutoTestSource {
    type Resources<'r> = ();
    type Output<'m> = output_msg!(IsotpPdu);

    fn new(_config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            iteration: 0,
            seq_idx: 0,
        })
    }

    fn process<'o>(&mut self, ctx: &CuContext, output: &mut Self::Output<'o>) -> CuResult<()> {
        let now = ctx.now();

        // Check if current iteration matches next entry in sequence
        if self.seq_idx < TEST_SEQUENCE.len() {
            let (target_iter, data) = TEST_SEQUENCE[self.seq_idx];
            if self.iteration == target_iter {
                let pdu = IsotpPdu::from_data(data);
                output.set_payload(pdu);
                output.tov = Tov::Time(now);
                self.seq_idx += 1;
            }
            // If iteration doesn't match, output has no payload (idle cycle)
        }

        self.iteration += 1;
        Ok(())
    }
}

/// Sink task that captures UDS responses.
#[derive(Reflect)]
#[reflect(from_reflect = false)]
pub struct AutoTestSink {
    response_count: usize,
}

impl Freezable for AutoTestSink {}

impl CuSinkTask for AutoTestSink {
    type Resources<'r> = ();
    type Input<'m> = input_msg!(IsotpPdu);

    fn new(_config: Option<&ComponentConfig>, _resources: Self::Resources<'_>) -> CuResult<Self>
    where
        Self: Sized,
    {
        Ok(Self { response_count: 0 })
    }

    fn process<'i>(&mut self, _ctx: &CuContext, input: &Self::Input<'i>) -> CuResult<()> {
        if input.payload().is_some() {
            self.response_count += 1;
        }
        Ok(())
    }
}
