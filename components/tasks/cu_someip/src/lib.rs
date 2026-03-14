//! # cu_someip — SOME/IP Tasks for Copper
//!
//! Provides SOME/IP UDP source, sink, and service discovery tasks
//! for the Copper runtime.
//!
//! ## Architecture
//!
//! ```text
//! SomeIpSource (UDP RX) → SomeIpRouter → SomeIpSink (UDP TX)
//! SomeIpSdMonitor (multicast listener) → notifies availability
//! ```
//!
//! ## Configuration (RON)
//! ```ron
//! (id: "someip_rx", type: "cu_someip::SomeIpSource", config: {
//!     "bind_addr": "0.0.0.0",
//!     "bind_port": 30509
//! })
//! (id: "someip_tx", type: "cu_someip::SomeIpSink", config: {
//!     "remote_addr": "192.168.1.100",
//!     "remote_port": 30509
//! })
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

mod router;
mod sd;
mod sink;
mod source;

pub use router::SomeIpRouter;
pub use sd::SomeIpSdMonitor;
pub use sink::SomeIpSink;
pub use source::SomeIpSource;

#[cfg(test)]
mod tests {
    use super::*;
    use cu_automotive_payloads::someip::{
        SomeIpMessage, SomeIpMessageType, SomeIpReturnCode, SomeIpServiceStatus,
    };
    use cu29::prelude::*;

    // ── Router Tests ──────────────────────────────────────────────

    fn make_router(services: &[u16]) -> SomeIpRouter {
        let svc_str = services
            .iter()
            .map(|s| alloc::format!("0x{:04X}", s))
            .collect::<alloc::vec::Vec<_>>()
            .join(",");
        let mut cfg = ComponentConfig::default();
        cfg.set::<alloc::string::String>("services".into(), svc_str);
        SomeIpRouter::new(Some(&cfg), ()).unwrap()
    }

    #[test]
    fn router_known_service_echoes() {
        let mut r = make_router(&[0x0100]);
        let req = SomeIpMessage::request(0x0100, 0x0001, 0x01, 0x01, &[42]);
        let input = CuMsg::<SomeIpMessage>::new(Some(req));
        let mut output = CuMsg::<SomeIpMessage>::default();
        let ctx = CuContext::new_with_clock();
        r.process(&ctx, &input, &mut output).unwrap();
        let resp = output.payload().unwrap();
        assert_eq!(resp.header.message_type, SomeIpMessageType::Response);
        assert_eq!(resp.payload_data(), &[42]);
    }

    #[test]
    fn router_unknown_service_errors() {
        let mut r = make_router(&[0x0100]);
        let req = SomeIpMessage::request(0x0200, 0x0001, 0x01, 0x01, &[]);
        let input = CuMsg::<SomeIpMessage>::new(Some(req));
        let mut output = CuMsg::<SomeIpMessage>::default();
        let ctx = CuContext::new_with_clock();
        r.process(&ctx, &input, &mut output).unwrap();
        let resp = output.payload().unwrap();
        assert_eq!(resp.header.message_type, SomeIpMessageType::Error);
        assert_eq!(resp.header.return_code, SomeIpReturnCode::UnknownService);
    }

    #[test]
    fn router_ignores_non_requests() {
        let mut r = make_router(&[0x0100]);
        let notif = SomeIpMessage::notification(0x0100, 0x8001, &[1, 2, 3]);
        let input = CuMsg::<SomeIpMessage>::new(Some(notif));
        let mut output = CuMsg::<SomeIpMessage>::default();
        let ctx = CuContext::new_with_clock();
        r.process(&ctx, &input, &mut output).unwrap();
        assert!(output.payload().is_none());
    }

    #[test]
    fn router_no_payload_no_output() {
        let mut r = make_router(&[0x0100]);
        let input = CuMsg::<SomeIpMessage>::default();
        let mut output = CuMsg::<SomeIpMessage>::default();
        let ctx = CuContext::new_with_clock();
        r.process(&ctx, &input, &mut output).unwrap();
        assert!(output.payload().is_none());
    }

    #[test]
    fn router_multiple_services() {
        let mut r = make_router(&[0x0100, 0x0200, 0x0300]);
        for svc in [0x0100, 0x0200, 0x0300] {
            let req = SomeIpMessage::request(svc, 0x0001, 0x01, 0x01, &[]);
            let input = CuMsg::<SomeIpMessage>::new(Some(req));
            let mut output = CuMsg::<SomeIpMessage>::default();
            let ctx = CuContext::new_with_clock();
            r.process(&ctx, &input, &mut output).unwrap();
            assert_eq!(
                output.payload().unwrap().header.message_type,
                SomeIpMessageType::Response
            );
        }
    }

    #[test]
    fn router_capacity_limit() {
        // Router supports 32 services; registering more should silently cap
        let many: alloc::vec::Vec<u16> = (1..=40).collect();
        let mut r = make_router(&many);
        let ctx = CuContext::new_with_clock();

        // Service 32 (index 31) should be registered
        let req = SomeIpMessage::request(32, 0x0001, 0x01, 0x01, &[]);
        let input = CuMsg::<SomeIpMessage>::new(Some(req));
        let mut output = CuMsg::<SomeIpMessage>::default();
        r.process(&ctx, &input, &mut output).unwrap();
        assert_eq!(
            output.payload().unwrap().header.message_type,
            SomeIpMessageType::Response
        );

        // Service 33 should NOT be registered (exceeded capacity)
        let req = SomeIpMessage::request(33, 0x0001, 0x01, 0x01, &[]);
        let input = CuMsg::<SomeIpMessage>::new(Some(req));
        let mut output = CuMsg::<SomeIpMessage>::default();
        r.process(&ctx, &input, &mut output).unwrap();
        assert_eq!(
            output.payload().unwrap().header.return_code,
            SomeIpReturnCode::UnknownService
        );
    }

    // ── SD Monitor Tests ──────────────────────────────────────────

    fn make_sd_payload(entries: &[(u8, u16, u16, u32)]) -> alloc::vec::Vec<u8> {
        // Build SD payload: 4 bytes flags + 4 bytes entries_len + 16*N entry bytes
        let entries_len = entries.len() * 16;
        let mut buf = alloc::vec![0u8; 8 + entries_len];
        buf[4..8].copy_from_slice(&(entries_len as u32).to_be_bytes());
        for (i, &(entry_type, svc_id, inst_id, ttl)) in entries.iter().enumerate() {
            let off = 8 + i * 16;
            buf[off] = entry_type;
            buf[off + 4..off + 6].copy_from_slice(&svc_id.to_be_bytes());
            buf[off + 6..off + 8].copy_from_slice(&inst_id.to_be_bytes());
            buf[off + 8] = 1; // major version
            // TTL is 3 bytes at offset 9..12
            let ttl_bytes = ttl.to_be_bytes();
            buf[off + 9] = ttl_bytes[1];
            buf[off + 10] = ttl_bytes[2];
            buf[off + 11] = ttl_bytes[3];
            // minor version at offset 12..16
            buf[off + 12..off + 16].copy_from_slice(&1u32.to_be_bytes());
        }
        buf
    }

    fn make_sd_msg(payload: &[u8]) -> SomeIpMessage {
        SomeIpMessage::request(0xFFFF, 0x8100, 0x00, 0x00, payload)
    }

    #[test]
    fn sd_offer_service_detected() {
        let mut sd = SomeIpSdMonitor::new(None, ()).unwrap();
        let payload = make_sd_payload(&[(0x01, 0x0100, 0x0001, 30)]);
        let msg = make_sd_msg(&payload);
        let input = CuMsg::<SomeIpMessage>::new(Some(msg));
        let mut output = CuMsg::<SomeIpServiceStatus>::default();
        let ctx = CuContext::new_with_clock();
        sd.process(&ctx, &input, &mut output).unwrap();
        let status = output.payload().unwrap();
        assert_eq!(status.service_id, 0x0100);
        assert!(status.available);
    }

    #[test]
    fn sd_stop_offer() {
        let mut sd = SomeIpSdMonitor::new(None, ()).unwrap();
        let ctx = CuContext::new_with_clock();

        // First offer
        let payload = make_sd_payload(&[(0x01, 0x0100, 0x0001, 30)]);
        let msg = make_sd_msg(&payload);
        let input = CuMsg::<SomeIpMessage>::new(Some(msg));
        let mut output = CuMsg::<SomeIpServiceStatus>::default();
        sd.process(&ctx, &input, &mut output).unwrap();
        assert!(output.payload().unwrap().available);

        // StopOffer (type 0x01 with TTL=0)
        let payload = make_sd_payload(&[(0x01, 0x0100, 0x0001, 0)]);
        let msg = make_sd_msg(&payload);
        let input = CuMsg::<SomeIpMessage>::new(Some(msg));
        let mut output = CuMsg::<SomeIpServiceStatus>::default();
        sd.process(&ctx, &input, &mut output).unwrap();
        let status = output.payload().unwrap();
        assert!(!status.available);
    }

    #[test]
    fn sd_ignores_non_sd_messages() {
        let mut sd = SomeIpSdMonitor::new(None, ()).unwrap();
        let msg = SomeIpMessage::request(0x0100, 0x0001, 0x01, 0x01, &[1, 2, 3]);
        let input = CuMsg::<SomeIpMessage>::new(Some(msg));
        let mut output = CuMsg::<SomeIpServiceStatus>::default();
        let ctx = CuContext::new_with_clock();
        sd.process(&ctx, &input, &mut output).unwrap();
        assert!(output.payload().is_none());
    }

    #[test]
    fn sd_parser_empty_payload() {
        let entries = SomeIpSdMonitor::parse_sd_entries(&[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn sd_parser_short_payload() {
        // Less than 12 bytes
        let entries = SomeIpSdMonitor::parse_sd_entries(&[0; 8]);
        assert!(entries.is_empty());
    }

    #[test]
    fn sd_parser_zero_entries() {
        let mut payload = [0u8; 12];
        // entries_len = 0
        payload[4..8].copy_from_slice(&0u32.to_be_bytes());
        let entries = SomeIpSdMonitor::parse_sd_entries(&payload);
        assert!(entries.is_empty());
    }

    #[test]
    fn sd_parser_boundary_no_overflow() {
        // entries_len claims 16 bytes but actual payload is only 8 bytes after header
        let mut payload = alloc::vec![0u8; 16]; // 8 header + 8 data (not enough for one entry)
        payload[4..8].copy_from_slice(&16u32.to_be_bytes()); // claims 16 bytes of entries
        let entries = SomeIpSdMonitor::parse_sd_entries(&payload);
        assert!(entries.is_empty()); // 8 < 16 so no complete entry
    }

    #[test]
    fn sd_duplicate_offer_no_change() {
        let mut sd = SomeIpSdMonitor::new(None, ()).unwrap();
        let ctx = CuContext::new_with_clock();

        // First offer → produces status
        let payload = make_sd_payload(&[(0x01, 0x0100, 0x0001, 30)]);
        let msg = make_sd_msg(&payload);
        let input = CuMsg::<SomeIpMessage>::new(Some(msg));
        let mut output = CuMsg::<SomeIpServiceStatus>::default();
        sd.process(&ctx, &input, &mut output).unwrap();
        assert!(output.payload().is_some());

        // Same offer again → no change, no output
        let payload = make_sd_payload(&[(0x01, 0x0100, 0x0001, 30)]);
        let msg = make_sd_msg(&payload);
        let input = CuMsg::<SomeIpMessage>::new(Some(msg));
        let mut output = CuMsg::<SomeIpServiceStatus>::default();
        sd.process(&ctx, &input, &mut output).unwrap();
        assert!(output.payload().is_none());
    }

    // ── Mock Source/Sink Tests ────────────────────────────────────

    #[cfg(feature = "mock")]
    #[test]
    fn mock_source_produces_messages() {
        let mut src = SomeIpSource::new(None, ()).unwrap();
        let ctx = CuContext::new_with_clock();
        src.preprocess(&ctx).unwrap();
        let mut output = CuMsg::<SomeIpMessage>::default();
        src.process(&ctx, &mut output).unwrap();
        assert!(output.payload().is_some());
    }

    #[cfg(feature = "mock")]
    #[test]
    fn mock_sink_accepts_message() {
        let mut sink = SomeIpSink::new(None, ()).unwrap();
        let msg = SomeIpMessage::request(0x0100, 0x0001, 0x01, 0x01, &[1]);
        let input = CuMsg::<SomeIpMessage>::new(Some(msg));
        let ctx = CuContext::new_with_clock();
        // Should not error
        sink.process(&ctx, &input).unwrap();
    }
}
