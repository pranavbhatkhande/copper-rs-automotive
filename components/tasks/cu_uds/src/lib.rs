//! # cu_uds — Unified Diagnostic Services (ISO 14229) for Copper
//!
//! Provides UDS server and client tasks that sit on top of ISO-TP.
//!
//! ## Architecture
//! ```text
//! CanSource → IsotpCodec → UdsServer → IsotpCodec → CanSink
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

mod client;
mod server;

pub use client::UdsClient;
pub use server::UdsServer;
