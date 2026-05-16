//! UDS Diagnostic Example
//!
//! Demonstrates a UDS diagnostic server pipeline:
//!   UdsTestSource → UdsServer → UdsResponseSink
//!
//! The test source generates diagnostic requests (DiagnosticSessionControl,
//! ReadDataByIdentifier, etc.) and the UDS server processes and responds.
//! This example uses mock mode — no real CAN hardware required.

pub mod tasks;

use cu29::prelude::*;
use std::fs;
use std::path::Path;

#[copper_runtime(config = "copperconfig.ron")]
struct UdsExampleApplication {}

const SLAB_SIZE: Option<usize> = Some(64 * 1024 * 1024);

fn main() {
    let logger_path = "logs/uds_example.copper";
    if let Some(parent) = Path::new(logger_path).parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).expect("Failed to create logs directory");
    }

    let mut application = UdsExampleApplication::builder()
        .with_log_path(logger_path, SLAB_SIZE)
        .expect("Failed to setup logger.")
        .build()
        .expect("Failed to create UDS example application.");

    println!("Starting UDS diagnostic example (TestSource → UdsServer → ResponseSink)...");
    if let Err(error) = application.run() {
        println!("Application ended: {error}");
    }
}
