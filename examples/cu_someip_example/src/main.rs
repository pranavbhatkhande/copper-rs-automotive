//! SOME/IP Service Example
//!
//! Demonstrates a SOME/IP pipeline: SomeIpSource → SomeIpRouter → SomeIpSink.
//! Uses mock mode — no real UDP sockets required.

use cu29::prelude::*;
use std::fs;
use std::path::Path;

#[copper_runtime(config = "copperconfig.ron")]
struct SomeIpExampleApplication {}

const SLAB_SIZE: Option<usize> = Some(64 * 1024 * 1024);

fn main() {
    let logger_path = "logs/someip_example.copper";
    if let Some(parent) = Path::new(logger_path).parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).expect("Failed to create logs directory");
    }

    let mut application = SomeIpExampleApplication::builder()
        .with_log_path(logger_path, SLAB_SIZE)
        .expect("Failed to setup logger.")
        .build()
        .expect("Failed to create SOME/IP example application.");

    println!("Starting SOME/IP example pipeline (SomeIpSource → SomeIpRouter → SomeIpSink)...");
    if let Err(error) = application.run() {
        println!("Application ended: {error}");
    }
}
