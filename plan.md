# Plan: Automotive Stack Production Hardening

## Problem Statement

Two independent audits (research.md, codex-research.md) converge on the same conclusion: copper-rs provides genuine structural determinism, but the copper-rs-automotive protocol stack is prototype-grade. Every critical finding from codex-research has been independently verified against source code.

**Verified critical gaps:**
- UDS S3/P2/P2* timers exist as fields but are never enforced
- ISO-TP ignores STmin from Flow Control and has no transport timeouts
- SOME/IP has config key drift, a SD parser boundary bug, and zero tests
- CAN has zero tests and silently drops read errors
- SecurityAccess has no attempt limiting
- Multiple panic-on-empty-input paths

**Scope constraint:** All changes in copper-rs-automotive only. No core/ modifications.

## Approach

Use `ctx.now()` (CuContext's RobotClock) for all timer implementations. This gives us:
- Real monotonic time in production
- Deterministic mock time in replay (verified by caterpillar test contract)
- Freezable state for keyframe snapshots (all timer fields are u64/CuTime)

The PID controller (`components/tasks/cu_pid`) already demonstrates this pattern successfully.

## Workstreams

### WS1: ISO-TP Protocol Compliance [Critical]

**Files:** `components/tasks/cu_isotp/src/lib.rs`

1. **Add STmin enforcement to TX path**
   - Extract `data[2]` (STmin) from Flow Control frames in the FC handler (~line 269)
   - Add `stmin: CuDuration` and `last_cf_time: CuTime` fields to `TxState::Sending`
   - In `tx_next_frame()`, check `now - last_cf_time >= stmin` before emitting CF
   - Pass `CuTime` into `tx_next_frame()` (currently takes no time parameter)
   - Parse STmin byte per ISO 15765-2: 0x00-0x7F = ms, 0xF1-0xF9 = 100-900μs

2. **Add transport timeout state machine**
   - Add `rx_start_time: CuTime` to `RxState::Receiving`
   - Add `tx_start_time: CuTime` and `fc_wait_start: CuTime` to `TxState::Sending`
   - Add configurable timeout constants: `N_BS_TIMEOUT` (1000ms default), `N_CR_TIMEOUT` (1000ms default)
   - In `process()`: check `now - rx_start_time > N_CR_TIMEOUT` → abort RX, transition to Idle
   - In `process()`: check `now - fc_wait_start > N_BS_TIMEOUT` when `waiting_fc` → abort TX
   - Emit `IsotpError` variants for timeout conditions

3. **Add Wait FC handling**
   - Currently `fs == 1` is a comment-only path
   - Implement: reset `fc_wait_start` timer, stay in waiting state
   - After N_WFTmax Wait frames, abort TX

4. **Expand tests to ≥20 cases**
   - Multi-frame RX reassembly (full path)
   - Block size enforcement (BS > 0 with multiple FC rounds)
   - STmin pacing verification (with mock time)
   - Timeout on stalled RX (no CF arrives)
   - Timeout on stalled TX (no FC arrives)
   - Wait FC handling (fs=1)
   - Overflow FC handling (fs=2) — existing but untested
   - Sequence number rollover (SN wraps 0xF → 0x0)
   - Malformed frame handling (truncated FF, bad SN)
   - Max PDU size boundary (4095 bytes)

### WS2: UDS Protocol Compliance [Critical]

**Files:** `components/tasks/cu_uds/src/server.rs`, `client.rs`, README.md

1. **Implement S3 session timeout**
   - Add `last_request_time: CuTime` field to `UdsServer`
   - In `handle()`: set `self.last_request_time = now` on every request
   - In `process()`: compute `elapsed = now - self.last_request_time`
   - If `elapsed > CuDuration::from_millis(self.session_timeout_ms)` and not in Default session → `reset_session()`
   - Remove dead `s3_remaining_ms` field — it's replaced by time-based logic
   - Pass `CuTime` into `process()` (available via `ctx.now()`)

2. **Implement P2/P2* server-side enforcement**
   - Add `request_received_time: Option<CuTime>` field
   - Add `pending_response: Option<(u8, Vec<u8>)>` or similar for deferred processing
   - On request receipt: store `request_received_time = Some(now)`
   - On each process() if response not yet ready:
     - If `elapsed > p2_server_ms` → send NRC 0x78 (RequestCorrectlyReceivedResponsePending)
     - If `elapsed > p2_star_server_ms` → send NRC 0x10 (GeneralReject) and clear pending
   - Remove `#[allow(dead_code)]` from p2 fields
   - Note: In copper-rs's synchronous model, "not yet ready" means a multi-iteration processing scenario. For the initial implementation, P2 timing can be advisory (log warning if exceeded) rather than interrupt-based.

3. **Implement client P2 timeout**
   - Add `request_sent_time: CuTime` field to `UdsClient`
   - In `process()`: when `self.awaiting` is true, compute `elapsed = now - request_sent_time`
   - If `elapsed > CuDuration::from_millis(self.p2_timeout_ms)` → set `self.awaiting = false`, emit timeout error
   - Remove `#[allow(dead_code)]` from `p2_timeout_ms`

4. **Add SecurityAccess hardening**
   - Add `security_attempt_count: u8` and `security_lockout_until: CuTime` fields
   - On invalid key: increment attempt count
   - If `attempt_count >= MAX_ATTEMPTS` (configurable, default 3): set lockout for `LOCKOUT_DURATION` (configurable, default 10s)
   - During lockout: return NRC 0x36 (ExceededNumberOfAttempts)
   - On successful auth: reset attempt counter
   - Remove hardcoded XOR key derivation — make it pluggable via a trait or closure

5. **Fix documentation mismatches**
   - Fix MAX_PENDING: change README "16" → "8", or increase constant to 16 (prefer increasing to 16 for real use)
   - Fix addressing: either implement source_addr/target_addr parsing, or remove from README examples
   - Align all config key names between code, README, and example RON files

6. **Expand tests to ≥25 cases**
   - S3 timeout fires and resets session (mock time)
   - S3 timeout does NOT fire in Default session
   - P2 timeout triggers NRC 0x78
   - Client P2 timeout clears awaiting state
   - SecurityAccess lockout after N failed attempts
   - SecurityAccess lockout expires after duration
   - All existing service handlers with edge cases
   - Session transitions (Default → Extended → Programming)
   - Queue full behavior
   - Invalid SID handling

### WS3: SOME/IP Correctness [Critical]

**Files:** `components/tasks/cu_someip/src/source.rs`, `sink.rs`, `router.rs`, `sd.rs`, README.md

1. **Fix config key alignment**
   - Decision: code keys are canonical (`bind_port`, `remote_addr`, `remote_port`)
   - Update README examples to use correct keys
   - Update `examples/cu_someip_example/copperconfig.ron` to use correct keys

2. **Fix SD parser boundary bug**
   - In `sd.rs` line 64: `8 + entries_len` is double-counting the offset
   - Fix: change bound to `entries_len` since `entry_data` is already `&payload[8..]`
   - Add bounds-check test with crafted payloads

3. **Fix router capacity mismatch**
   - Decision: increase `MAX_SERVICES` to 32 (aligning with README), or update README to 16
   - Prefer increasing to 32 — 16 is too low for real automotive service meshes

4. **Add socket error handling**
   - Source: surface `recv` errors through `CuResult::Err` or at minimum log them
   - Sink: check `sendto` return value, surface errors
   - Do NOT panic on socket errors — return `CuResult::Err`

5. **Add unit tests (≥15 cases)**
   - Source config parsing (correct keys, defaults)
   - Sink config parsing (correct keys, defaults)
   - Router service registration and lookup
   - Router capacity limit
   - SD entry parsing (valid entries)
   - SD parser boundary (crafted overflow payload)
   - SD parser with zero entries
   - SOME/IP header serialization/deserialization round-trip
   - Request/response matching

### WS4: CAN Hardening [High]

**Files:** `components/tasks/cu_can/src/lib.rs`, `socketcan.rs`

1. **Surface read errors**
   - Change `read_frame_nonblocking` to return `Result<Option<CanFrame>, CuError>` (or similar)
   - Distinguish "no data" (EAGAIN/EWOULDBLOCK) from actual errors
   - CanSource `preprocess()` should propagate or log errors

2. **Fix lint warnings**
   - Remove unused import flagged by `cargo test` warning

3. **Add unit tests (≥10 cases)**
   - CanFilter with matching ID
   - CanFilter with non-matching ID
   - CanFilter with multiple IDs
   - CanFilter with empty config (pass-all)
   - CAN frame encoding/decoding round-trip
   - Mock source frame generation
   - Extended vs standard frame handling

### WS5: Safety Guards [High]

**Files:** various

1. **Fix toyota_checksum empty-data panic**
   - `toyota_checksum()`: return 0 or error for empty data
   - `apply_toyota_checksum()`: return early or error for empty/single-byte data
   - Add test for empty input

2. **Audit all `unwrap()`/`expect()` in automotive components**
   - Replace with `?` or explicit error handling
   - No panics allowed on user-facing data paths

3. **Add input validation to all task `process()` methods**
   - Check payload presence before accessing
   - Validate frame lengths before slicing
   - Return `CuResult::Err` with descriptive errors, never panic

### WS6: Documentation Alignment [Medium]

1. **Update all READMEs**
   - cu_uds/README.md: fix MAX_PENDING claim, remove or implement address config
   - cu_someip/README.md: fix config key names, fix MAX_SERVICES claim
   - cu_isotp/README.md: add timeout/STmin behavior docs
   - cu_can/README.md: document error handling behavior

2. **Update copperconfig.ron examples**
   - Ensure all example RON files use config keys that actually exist in code

3. **Update AUTOMOTIVE_HIGHLIGHTS.md** with post-hardening capabilities

### WS7: Automotive Determinism Regression Test [High]

**New directory:** `examples/cu_automotive_determinism_test/`

Model after `examples/cu_caterpillar/src/determinism_test.rs`:

1. **Build a minimal automotive pipeline**
   - Mock CAN source → CanFilter → IsotpCodec → UdsServer → IsotpCodec → Mock CAN sink
   - Uses mock clock with deterministic stepping
   - Sim mode callbacks for source/sink stubs

2. **Enforce the four-part contract**
   - record_A.copperlists == record_B.copperlists
   - record_A.keyframes == record_B.keyframes
   - record_A.copperlists == resim(A).copperlists
   - record_A.keyframes == resim(A).keyframes

3. **Verify timer determinism specifically**
   - Include scenarios where S3 timeout fires
   - Include scenarios where ISO-TP timeout fires
   - Verify timer-dependent state transitions produce identical results

4. **Add to CI** (just target or cargo test)

## Dependency Order

```
WS5 (safety guards) — no dependencies, do first
  ↓
WS1 (ISO-TP) — foundational protocol, UDS depends on it
  ↓
WS2 (UDS) — depends on ISO-TP being correct
  ↓
WS3 (SOME/IP) — independent of UDS/ISO-TP
WS4 (CAN) — independent of UDS/ISO-TP
  ↓
WS6 (docs) — after all code changes
WS7 (determinism test) — after all protocol changes
```

Parallelizable: WS3 + WS4 can run in parallel with WS2. WS5 can start immediately.

## Architecture Decisions

1. **Timer pattern:** All protocol timers use `ctx.now()` → `CuTime` stored in task state. Elapsed time computed as `now - stored_time`. This is automatically deterministic in replay via mock clock.

2. **Error handling:** Never panic. All automotive task methods return `CuResult<()>`. Protocol errors produce error variants in output messages, not Rust panics.

3. **Configurability:** Timer values are configurable via RON config (`ComponentConfig`). Defaults match ISO standards (e.g., P2=50ms, P2*=5000ms, N_Bs=1000ms, S3=5000ms).

4. **State serialization:** All new timer fields are `CuTime`/`CuDuration` (u64 wrappers) → automatically Freezable via bincode derive. No special serialization needed.

5. **Testing:** All timer tests use `CuContext::new_with_clock()` or mock clock. No dependency on wall-clock time. Tests are deterministic.

6. **Constants vs config:** Protocol-mandated limits (MAX_PDU_SIZE, frame sizes) stay as constants. Tunable parameters (timeouts, queue depths) go in RON config.

## Success Criteria

- [ ] All ISO-TP timing tests pass with mock clock
- [ ] UDS S3/P2/P2* timeout behavior is testable and tested
- [ ] UDS client timeout prevents infinite waits
- [ ] SecurityAccess has attempt limiting
- [ ] SOME/IP config keys match between code and docs
- [ ] SD parser handles boundary cases correctly
- [ ] Zero panics possible from external input in any automotive task
- [ ] Automotive determinism test passes the 4-part contract
- [ ] `cargo test` passes for all automotive crates with no warnings
- [ ] All README documentation matches implementation
