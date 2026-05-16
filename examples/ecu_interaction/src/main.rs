// ECU Interaction Example.
//
// This application simulates a simple CAN network between two ECUs:
// a Body ECU and a Meter ECU.

pub mod tasks;

use cu29::prelude::*;

#[copper_runtime(config = "copperconfig.ron")]
struct EcuInteractionApp {}

fn main() {
    let logger_path = "logs/ecu_interaction.copper";
    let slab_size = Some(16 * 1024 * 1024);

    let mut application = EcuInteractionApp::builder()
        .with_log_path(logger_path, slab_size)
        .expect("Failed to setup logger.")
        .build()
        .expect("Failed to build application.");

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  ECU Interaction Simulation (Body <-> Meter)               ║");
    println!("║  Communication: CAN (Body: 0x456 -> Meter | Meter: 0x123 -> Body) ║");
    println!("║  Press Ctrl+C to stop                                      ║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    if let Err(error) = application.run() {
        println!("Simulation ended: {error}");
    }
}
