//! Automotive Determinism Test
//!
//! Verifies that automotive protocol tasks (UDS server with timers)
//! produce bit-identical results across independent runs, proving
//! the stack is fully deterministic under Copper's mock clock.

pub mod tasks;

use cu29::prelude::*;
use cu29_helpers::basic_copper_setup;
use std::fs;
use std::path::{Path, PathBuf};

#[copper_runtime(config = "copperconfig.ron")]
struct AutoDetApp {}

const SLAB_SIZE: Option<usize> = Some(64 * 1024 * 1024);

fn main() {
    let logger_path = "logs/automotive_det.copper";
    if let Some(parent) = Path::new(logger_path).parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).expect("Failed to create logs directory");
    }

    let copper_ctx = basic_copper_setup(&PathBuf::from(logger_path), SLAB_SIZE, true, None)
        .expect("Failed to setup logger.");
    let mut application = AutoDetAppBuilder::new()
        .with_context(&copper_ctx)
        .build()
        .expect("Failed to create application.");

    println!("Starting automotive determinism test...");
    if let Err(error) = application.run() {
        println!("Application ended: {error}");
    }
}

#[cfg(all(test, feature = "determinism_ci"))]
mod determinism_test;
