//! Automotive Determinism Contract Test
//!
//! Proves the four-part determinism contract for the automotive stack:
//!   1) record A copperlists == record B copperlists
//!   2) record A keyframes == record B keyframes
//!   3) record A copperlists == resim(A) copperlists
//!   4) record A keyframes == resim(A) keyframes
//!
//! The pipeline exercises UDS session management, S3 timeout (timer-dependent),
//! SecurityAccess, DID operations, and deliberately triggers timeout paths.
//! All timer behavior is deterministic via mock clock.

use cu_automotive_payloads::isotp::IsotpPdu;
use cu29::bincode;
use cu29::prelude::*;
use cu29_export::{copperlists_reader, keyframes_reader};
use cu29_helpers::basic_copper_setup;

use crate::tasks;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

static DET_LOCK: Mutex<()> = Mutex::new(());
static RUN_ID: AtomicUsize = AtomicUsize::new(0);

const DET_LOG_SLAB_SIZE: Option<usize> = Some(64 * 1024 * 1024);

#[copper_runtime(config = "copperconfig.ron", sim_mode = true)]
struct AutoDetSimApp {}

fn out_root_dir() -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    base.join("determinism_tests")
}

fn fresh_case_dir(case: &str) -> PathBuf {
    let rid = RUN_ID.fetch_add(1, Ordering::SeqCst);
    let dir = out_root_dir().join(format!("{}_pid{}_{}", case, std::process::id(), rid));
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir).expect("failed to create determinism output dir");
    dir
}

/// Record a deterministic run with mock clock.
/// dt_ticks controls time advancement per iteration.
/// With session_timeout_ms=5000 and dt_ticks=500_000_000 (500ms),
/// the gap from iteration 15→30 (15 * 500ms = 7500ms) exceeds S3 timeout.
fn record_run(log_base: &Path, iterations: usize, dt_ticks: u64) -> CuResult<()> {
    if let Some(parent) = log_base.parent() {
        fs::create_dir_all(parent).ok();
    }

    let (clock, clock_mock) = RobotClock::mock();
    let ctx = basic_copper_setup(log_base, DET_LOG_SLAB_SIZE, false, Some(clock.clone()))?;

    let clock_for_sim = clock.clone();
    let mut source_iteration: usize = 0;
    let mut source_seq_idx: usize = 0;

    let mut sim_callback = move |step: default::SimStep| -> SimOverride {
        use default::SimStep::*;
        match step {
            // Source: reproduce AutoTestSource logic deterministically
            Src(CuTaskCallbackState::Process(_, output)) => {
                let now = clock_for_sim.now();
                if source_seq_idx < tasks::TEST_SEQUENCE.len() {
                    let (target_iter, data) = tasks::TEST_SEQUENCE[source_seq_idx];
                    if source_iteration == target_iter {
                        let pdu = IsotpPdu::from_data(data);
                        output.set_payload(pdu);
                        output.tov = Tov::Time(now);
                        source_seq_idx += 1;
                    }
                }
                output.metadata.process_time.start = now.into();
                output.metadata.process_time.end = now.into();
                source_iteration += 1;
                SimOverride::ExecutedBySim
            }
            Src(_) => SimOverride::ExecutedBySim,

            // Sink: capture output deterministically (no hardware)
            Sink(CuTaskCallbackState::Process(input, output)) => {
                let now = clock_for_sim.now();
                output.tov = input.tov;
                output.metadata.process_time.start = now.into();
                output.metadata.process_time.end = now.into();
                SimOverride::ExecutedBySim
            }
            Sink(_) => SimOverride::ExecutedBySim,

            // UDS server: let runtime execute
            _ => SimOverride::ExecuteByRuntime,
        }
    };

    let mut app = AutoDetSimAppBuilder::new()
        .with_context(&ctx)
        .with_sim_callback(&mut sim_callback)
        .build()
        .expect("failed to build app");

    app.start_all_tasks(&mut sim_callback)
        .expect("failed to start tasks");

    for i in 0..iterations {
        clock_mock.set_value(dt_ticks.saturating_mul(i as u64));
        app.run_one_iteration(&mut sim_callback)
            .expect("run_one_iteration failed");
    }

    app.stop_all_tasks(&mut sim_callback)
        .expect("failed to stop tasks");

    Ok(())
}

fn read_copperlist_stream_encoded(log_base: &Path) -> CuResult<Vec<Vec<u8>>> {
    let UnifiedLogger::Read(dl) = UnifiedLoggerBuilder::new()
        .file_base_name(log_base)
        .build()
        .expect("failed to open log for read")
    else {
        panic!("expected read logger");
    };

    let mut io_reader = UnifiedLoggerIOReader::new(dl, UnifiedLogType::CopperList);
    let iter = copperlists_reader::<default::CuStampedDataSet>(&mut io_reader);

    let mut out = Vec::new();
    for cl in iter {
        let bytes = bincode::encode_to_vec(cl, bincode::config::standard())
            .expect("failed to bincode-encode copperlist");
        out.push(bytes);
    }
    Ok(out)
}

fn read_keyframe_stream_encoded(log_base: &Path) -> CuResult<Vec<Vec<u8>>> {
    let UnifiedLogger::Read(dl) = UnifiedLoggerBuilder::new()
        .file_base_name(log_base)
        .build()
        .expect("failed to open log for read")
    else {
        panic!("expected read logger");
    };

    let mut io_reader = UnifiedLoggerIOReader::new(dl, UnifiedLogType::FrozenTasks);
    let iter = keyframes_reader(&mut io_reader);

    let mut out = Vec::new();
    for kf in iter {
        let bytes = bincode::encode_to_vec(kf, bincode::config::standard())
            .expect("failed to bincode-encode keyframe");
        out.push(bytes);
    }
    Ok(out)
}

fn resim_one_copperlist(
    app: &mut AutoDetSimApp,
    robot_clock_mock: &mut RobotClockMock,
    copper_list: CopperList<default::CuStampedDataSet>,
) {
    use default::SimStep::*;

    let msgs = &copper_list.msgs;

    // Sync clock to the recorded source output time.
    let CuDuration(ticks) = msgs.get_src_output().metadata.process_time.start.unwrap();
    robot_clock_mock.set_value(ticks);

    let mut cb = move |step: default::SimStep| -> SimOverride {
        match step {
            // Inject recorded source output
            Src(CuTaskCallbackState::Process(_, output)) => {
                *output = msgs.get_src_output().clone();
                SimOverride::ExecutedBySim
            }
            Src(_) => SimOverride::ExecutedBySim,

            // Stub sink
            Sink(CuTaskCallbackState::Process(input, output)) => {
                let now = robot_clock_mock.now();
                output.tov = input.tov;
                output.metadata.process_time.start = now.into();
                output.metadata.process_time.end = now.into();
                SimOverride::ExecutedBySim
            }
            Sink(_) => SimOverride::ExecutedBySim,

            // UDS server: let runtime execute
            _ => SimOverride::ExecuteByRuntime,
        }
    };

    app.run_one_iteration(&mut cb)
        .expect("resim run_one_iteration failed");
}

fn resim_run(input_log_base: &Path, output_log_base: &Path) -> CuResult<()> {
    if let Some(parent) = output_log_base.parent() {
        fs::create_dir_all(parent).ok();
    }

    let (clock, mut clock_mock) = RobotClock::mock();
    let ctx = basic_copper_setup(
        output_log_base,
        DET_LOG_SLAB_SIZE,
        false,
        Some(clock.clone()),
    )?;

    fn init_cb(_step: default::SimStep) -> SimOverride {
        SimOverride::ExecuteByRuntime
    }

    let mut app = AutoDetSimAppBuilder::new()
        .with_context(&ctx)
        .with_sim_callback(&mut init_cb)
        .build()
        .expect("failed to build resim app");

    app.start_all_tasks(&mut init_cb)
        .expect("failed to start tasks (resim)");

    let UnifiedLogger::Read(dl) = UnifiedLoggerBuilder::new()
        .file_base_name(input_log_base)
        .build()
        .expect("failed to open input log for resim")
    else {
        panic!("expected read logger for input");
    };

    let mut io_reader = UnifiedLoggerIOReader::new(dl, UnifiedLogType::CopperList);
    let iter = copperlists_reader::<default::CuStampedDataSet>(&mut io_reader);

    for cl in iter {
        resim_one_copperlist(&mut app, &mut clock_mock, cl);
    }

    app.stop_all_tasks(&mut init_cb)
        .expect("failed to stop tasks (resim)");

    Ok(())
}

fn assert_streams_equal(label_a: &str, a: &[Vec<u8>], label_b: &str, b: &[Vec<u8>]) {
    assert_eq!(
        a.len(),
        b.len(),
        "determinism failure: stream length differs ({}={}, {}={})",
        label_a,
        a.len(),
        label_b,
        b.len()
    );
    for (i, (item_a, item_b)) in a.iter().zip(b.iter()).enumerate() {
        if item_a != item_b {
            panic!(
                "determinism failure: mismatch at copperlist index {} ({} vs {})",
                i, label_a, label_b
            );
        }
    }
}

#[test]
fn automotive_determinism_record_and_resim() {
    let _guard = DET_LOCK.lock().unwrap();

    let iterations: usize = std::env::var("COPPER_DETERMINISM_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64);

    // 500ms per iteration. S3 timeout = 5000ms.
    // Gap from iter 15→30 = 15 iterations * 500ms = 7500ms → triggers S3 timeout.
    let dt_ticks: u64 = std::env::var("COPPER_DETERMINISM_DT_TICKS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500_000_000);

    let case_dir = fresh_case_dir("cu_automotive_det");
    let a_base = case_dir.join("record_a.copper");
    let b_base = case_dir.join("record_b.copper");
    let r_base = case_dir.join("resim_a.copper");

    // 1) Record A and B independently
    record_run(&a_base, iterations, dt_ticks).expect("record A failed");
    record_run(&b_base, iterations, dt_ticks).expect("record B failed");

    let a_stream = read_copperlist_stream_encoded(&a_base).expect("read A failed");
    let b_stream = read_copperlist_stream_encoded(&b_base).expect("read B failed");
    let a_keyframes = read_keyframe_stream_encoded(&a_base).expect("read A keyframes failed");
    let b_keyframes = read_keyframe_stream_encoded(&b_base).expect("read B keyframes failed");

    // 2) A == B (copperlists + keyframes)
    assert_streams_equal("record_a", &a_stream, "record_b", &b_stream);
    assert!(
        !a_keyframes.is_empty(),
        "determinism precondition failure: expected keyframes to be emitted"
    );
    assert_streams_equal("record_a_kf", &a_keyframes, "record_b_kf", &b_keyframes);

    // 3) Resim(A)
    resim_run(&a_base, &r_base).expect("resim(A) failed");
    let r_stream = read_copperlist_stream_encoded(&r_base).expect("read resim failed");
    let r_keyframes = read_keyframe_stream_encoded(&r_base).expect("read resim keyframes failed");

    // 4) A == resim(A) (copperlists + keyframes)
    assert_streams_equal("record_a", &a_stream, "resim_a", &r_stream);
    assert_streams_equal("record_a_kf", &a_keyframes, "resim_a_kf", &r_keyframes);

    let _ = fs::remove_dir_all(case_dir);
}
