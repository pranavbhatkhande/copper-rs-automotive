# COPPER-RS-AUTOMOTIVE: KEY AUTOMOTIVE COMPONENTS

## Quick Reference: Automotive-Specific Files

### New Automotive Tasks
```
components/tasks/
├── cu_can/          (CAN ISO 11898) — SocketCAN + mock mode + error handling
├── cu_isotp/        (ISO 15765-2 Transport Layer) — STmin, timeouts, Wait FC
├── cu_someip/       (AUTOSAR SOME/IP networking) — SD parsing, error handling
└── cu_uds/          (ISO 14229 Diagnostics) — S3 timeout, SecurityAccess lockout
```

### Automotive Payloads (Single Crate)
```
components/payloads/cu_automotive_payloads/
├── src/can.rs       (CAN 2.0B, CAN FD, CanFrame, CanId)
├── src/isotp.rs     (ISO-TP PDU, addressing modes, flow control)
├── src/someip.rs    (SOME/IP headers, messages, service discovery)
├── src/uds.rs       (UDS services, diagnostics, negative response codes)
└── src/lib.rs       (Module exports)
```

### Automotive Examples
```
examples/
├── cu_can_example/          → Basic CAN pipeline demo
├── cu_someip_example/       → SOME/IP routing demo
├── cu_uds_example/          → Diagnostic server demo
└── cu_vehicle_sim/          → Toyota TSS2 ADAS CAN network sim (35 messages)
```

---

## Protocol Hardening Summary

All automotive protocol tasks have been audited and hardened for determinism and ISO compliance. Key improvements:

### Timers & Determinism
All protocol timers use `ctx.now()` → `CuTime`, which is:
- Real monotonic time in production
- Deterministic mock time during replay/simulation
- Automatically serializable for keyframe snapshots

### Error Handling
- No panics on user-facing data paths
- All tasks return `CuResult<()>` with descriptive errors
- Socket errors are surfaced, not silently dropped

### Test Coverage
- **CAN:** 11 unit tests (filter matching, masking, mock mode)
- **ISO-TP:** 19 unit tests (STmin, timeouts, reassembly, Wait FC)
- **SOME/IP:** 16 unit tests (routing, SD parsing, capacity)
- **UDS:** 21 unit tests (S3 timeout, SecurityAccess lockout, session management)
- **Vehicle Sim:** 9 unit tests (checksums, empty-input guards)

---

## Automotive Task Implementations

### 1. CAN Task (cu_can)
**File:** `/components/tasks/cu_can/src/lib.rs` + `socketcan.rs`

```rust
pub struct CanSource { ... }      // SocketCAN reader
pub struct CanSink { ... }        // SocketCAN writer
pub struct CanFilter { ... }      // ID-based frame filter
```

**Features:**
- Real SocketCAN interface support (Linux)
- Mock mode for testing without hardware
- Non-blocking I/O with error distinction (EAGAIN vs real errors)
- Configurable ID filter with mask

---

### 2. ISO-TP Task (cu_isotp)
**File:** `/components/tasks/cu_isotp/src/lib.rs`

**Responsibilities:**
- CAN frame segmentation (multi-frame transfers)
- Reassembly from consecutive frames
- Flow control sequencing (ContinueToSend, Wait, Overflow)
- **STmin enforcement** — Parses STmin byte per ISO 15765-2 (ms and 100μs ranges)
- **Transport timeouts** — N_Bs (1000ms) and N_Cr (1000ms) abort stalled transfers
- **Wait FC handling** — fs=1 with N_WFTmax counter
- Block size handling
- Configurable timeouts via RON

---

### 3. SOME/IP Task (cu_someip)
**File:** `/components/tasks/cu_someip/src/`

Four task types:
```rust
pub struct SomeIpSource { ... }     // UDP RX
pub struct SomeIpSink { ... }       // UDP TX  
pub struct SomeIpRouter { ... }     // Request/response routing (32 services max)
pub struct SomeIpSdMonitor { ... }  // Service discovery
```

**Protocol Details:**
- 16-byte fixed header (Service ID, Method ID, Client ID, Session ID, etc.)
- 1400-byte payload buffer (fixed-size, no heap)
- 10 message types (Request, Response, Error, with TP variants)
- Service discovery on port 30490 (UDP multicast)
- Socket error handling on recv/sendto

**Config keys:** `bind_port`, `remote_addr`, `remote_port`

---

### 4. UDS Task (cu_uds)
**File:** `/components/tasks/cu_uds/src/`

```rust
pub struct UdsServer { ... }    // Diagnostic server
pub struct UdsClient { ... }    // Diagnostic client
```

**Implemented UDS Services:**
- DiagnosticSessionControl (0x10) — Default, Programming, Extended
- EcuReset (0x11)
- SecurityAccess (0x27) — Attempt limiting + lockout timer
- TesterPresent (0x3E) — Keep-alive
- ReadDataByIdentifier (0x22) — DIDs
- WriteDataByIdentifier (0x2E)
- RoutineControl (0x31)
- ClearDTC (0x14)

**Protocol Compliance:**
- S3 session timeout (5000ms default) — auto-reverts to Default session
- P2/P2\* server timing parameters (configurable)
- Client P2 timeout (1000ms default) — prevents infinite waits
- SecurityAccess attempt limiting (3 attempts, 10s lockout)
- Sub-function suppress-positive-response only on applicable SIDs

---

## Automotive Payloads in Detail

### CAN Payloads (317 lines)
```rust
pub enum CanId {
    Standard(u16),    // 11-bit (0x000 – 0x7FF)
    Extended(u32),    // 29-bit (0x0000_0000 – 0x1FFF_FFFF)
}

pub struct CanFrame {
    pub id: CanId,
    pub dlc: u8,
    pub data: [u8; 8],         // Fixed 8-byte buffer
    pub flags: CanFlags,        // RTR, ERR flags
}

pub struct CanFdFrame {
    pub id: CanId,
    pub dlc: u8,
    pub data: [u8; 64],         // Fixed 64-byte buffer (CAN FD)
    pub flags: CanFdFlags,      // BRS, ESI flags
}

pub struct CanFrameBatch<const N: usize> {
    pub frames: [CanFrame; N],
    pub len: usize,
}
```

**Zero-Allocation Design:**
- Fixed-size arrays on stack
- Only `len` field indicates valid entries
- No Vec, no heap allocation
- Optimized for hot path

---

### ISO-TP Payloads (212 lines)
```rust
pub const ISOTP_MAX_PDU_SIZE: usize = 4095;

pub struct IsotpPdu {
    pub source_addr: u32,
    pub target_addr: u32,
    pub addressing_mode: IsotpAddressingMode,
    pub data: [u8; ISOTP_MAX_PDU_SIZE],
    pub len: u16,
}

pub enum IsotpFrameType {
    SingleFrame,        // Entire message in one CAN frame
    FirstFrame,         // Multi-frame start
    ConsecutiveFrame,   // Continuation
    FlowControl,        // Receiver controls pacing
}

pub enum IsotpAddressingMode {
    Normal,         // CAN ID identifies channel
    NormalFixed,    // 29-bit CAN ID embeds source/target
    Extended,       // First data byte is address extension
    Mixed11,        // Mixed 11-bit
    Mixed29,        // Mixed 29-bit
}

pub struct FlowControlParams {
    pub status: FlowStatus,     // ContinueToSend, Wait, Overflow
    pub block_size: u8,         // Frames before next FC (0 = all)
    pub st_min: u8,             // Separation time (ms or μs)
}
```

**Key Property:** 4095-byte max payload fits in copper zero-alloc design

---

### UDS Payloads (428 lines)
```rust
pub struct UdsRequest {
    pub service_id: u8,         // SID (e.g., 0x22 for ReadDID)
    pub sub_function: u8,       // Optional sub-function
    pub has_sub_function: bool,
    pub target_addr: u32,       // ECU logical address
    pub data: [u8; UDS_MAX_PAYLOAD_SIZE],
    pub data_len: u16,
}

pub struct UdsResponse {
    pub service_id: u8,         // Positive response SID+0x40, or 0x7F for negative
    pub nrc: Nrc,               // Negative Response Code
    pub is_negative: bool,
    pub source_addr: u32,       // ECU source address
    pub data: [u8; UDS_MAX_PAYLOAD_SIZE],
    pub data_len: u16,
}

pub const UDS_MAX_PAYLOAD_SIZE: usize = 4093;

pub enum UdsSessionType {
    Default = 0x01,
    Programming = 0x02,
    Extended = 0x03,
}

pub enum Nrc {
    PositiveResponse = 0x00,
    GeneralReject = 0x10,
    ServiceNotSupported = 0x11,
    SecurityAccessDenied = 0x33,
    Timeout = 0x78,
    // ... 21 more
}

pub type Did = u16;  // Data Identifier

pub mod did_ranges {
    pub const VIN: u16 = 0xF190;
    pub const ECU_SERIAL: u16 = 0xF18C;
    pub const ECU_HW_VERSION: u16 = 0xF191;
    pub const ECU_SW_VERSION: u16 = 0xF195;
    // ... standard DID ranges
}
```

---

### SOME/IP Payloads (417 lines)
```rust
pub const SOMEIP_MAX_PAYLOAD_SIZE: usize = 1400;
pub const SOMEIP_HEADER_SIZE: usize = 16;  // Fixed on-wire
pub const SOMEIP_SD_PORT: u16 = 30490;

pub enum SomeIpMessageType {
    Request = 0x00,
    RequestNoReturn = 0x01,
    Notification = 0x02,
    Response = 0x80,
    Error = 0x81,
    TpRequest = 0x20,           // Transport Protocol variants
    TpRequestNoReturn = 0x21,
    TpNotification = 0x22,
    TpResponse = 0xA0,
    TpError = 0xA1,
}

pub enum SomeIpReturnCode {
    Ok = 0x00,
    NotOk = 0x01,
    UnknownService = 0x02,
    UnknownMethod = 0x03,
    NotReady = 0x04,
    Timeout = 0x06,
    // ... 4 more
}

pub struct SomeIpHeader {
    pub service_id: u16,        // 16-bit service identifier
    pub method_id: u16,         // 16-bit method/event ID
    pub length: u32,            // Payload + 8 bytes of header
    pub client_id: u16,
    pub session_id: u16,
    pub protocol_version: u8,   // Always 0x01
    pub interface_version: u8,
    pub message_type: SomeIpMessageType,
    pub return_code: SomeIpReturnCode,
}

pub struct SomeIpMessage {
    pub header: SomeIpHeader,
    pub payload: [u8; SOMEIP_MAX_PAYLOAD_SIZE],  // Fixed 1400 bytes
    pub payload_len: u16,       // Valid bytes
}
```

**Wire Format (16-byte header, big-endian):**
```
Bytes 0-3:  Service ID (16 bits) | Method ID (16 bits)
Bytes 4-7:  Length (32-bit big-endian)
Bytes 8-9:  Client ID (16 bits)
Bytes 10-11: Session ID (16 bits)
Byte 12:    Protocol Version (0x01)
Byte 13:    Interface Version
Byte 14:    Message Type
Byte 15:    Return Code
```

---

## Example Application: cu_vehicle_sim

**Most Complex Automotive Example** — Full Toyota TSS2 ADAS Simulation

**Application:** `/examples/cu_vehicle_sim/src/`

### Modules:
1. **dbc_generated.rs** — Auto-generated CAN signal definitions (from DBC)
2. **signal_pack.rs** — Encoding/decoding of physical signal values to CAN bytes
3. **toyota_checksum.rs** — Toyota proprietary checksum algorithms
4. **ecu_radar.rs** — Radar ECU simulation (ToyotaRadarEcu Copper task)
5. **bus_spy.rs** — CAN frame analyzer & validator (CanBusSpy Copper task)

### Simulation Details:
- **DBC File:** `toyota_tss2_adas.dbc` (34 CAN messages defined)
- **Message Types:**
  - TRACK_A_0..15 — Primary radar track data (distance, speed, lateral offset)
  - TRACK_B_0..15 — Secondary radar track data (acceleration, confidence)
  - NEW_MSG_1, NEW_MSG_2 — Miscellaneous ADAS signals
- **Features:**
  - Proper Toyota checksums (not CRC32, but Toyota-specific algorithm)
  - Auto-incrementing counters per message
  - Configurable active track count (default: 6)
  - Base speed simulation (default: 100 kph)
  - Frame validation by bus spy

### Config:
```ron
(id: "radar_ecu", 
 type: "ecu_radar::ToyotaRadarEcu", 
 config: {"active_tracks": 6, "base_speed_kph": 100.0}),
(id: "bus_spy", 
 type: "bus_spy::CanBusSpy", 
 config: {"verbose": true, "summary_interval": 340}),
```

### Task Graph:
```
ToyotaRadarEcu (source) → CanBusSpy (sink)
   ↓
   Outputs 34 TRACK_A/TRACK_B messages per cycle
   ↓
   Bus spy validates checksums, counters, signal ranges
```

---

## Automotive Networking Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    COPPER AUTOMOTIVE STACK                  │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │   APPLICATION LAYER (UDS Services, SomeIP Methods) │   │
│  └────────────────────┬────────────────────────────────┘   │
│                       │                                     │
│  ┌────────────────────▼────────────────────────────────┐   │
│  │     UDS Layer (ISO 14229) & SOME/IP Layer           │   │
│  │     • Request/Response handling                      │   │
│  │     • Session management                            │   │
│  │     • Negative response codes                       │   │
│  └────────────────────┬────────────────────────────────┘   │
│                       │                                     │
│  ┌────────────────────▼────────────────────────────────┐   │
│  │     ISO-TP Transport Layer (ISO 15765-2)            │   │
│  │     • Segmentation (multi-frame)                    │   │
│  │     • Reassembly                                    │   │
│  │     • Flow control                                  │   │
│  └────────────────────┬────────────────────────────────┘   │
│                       │                                     │
│  ┌────────────────────▼────────────────────────────────┐   │
│  │  CAN Bus Layer (ISO 11898)                          │   │
│  │  • CanFrame / CanFdFrame                            │   │
│  │  • Hardware: SocketCAN (Linux)                      │   │
│  │  • Testing: Mock mode                              │   │
│  └────────────────────┬────────────────────────────────┘   │
│                       │                                     │
│                    (Physical Bus)                          │
│                                                             │
└─────────────────────────────────────────────────────────────┘

UDP/Network Layer (for SOME/IP)
      ↓
┌────────────────────────────────────────────┐
│  SomeIpSource → SomeIpRouter → SomeIpSink  │
└────────────────────────────────────────────┘
```

---

## Zero-Allocation Design Pattern

All automotive payloads follow copper-rs' zero-alloc principle:

```rust
// ❌ NOT USED (heap allocation)
pub data: Vec<u8>,

// ✅ USED (fixed-size buffer, stack allocation)
pub data: [u8; 4096],
pub len: u16,

// Usage:
let payload = IsotpPdu::new(0x7E0, 0x7E8, &my_data);
let valid_data = &payload.data[..payload.len as usize];
```

**Benefits:**
- Deterministic memory usage (no GC pauses)
- Microsecond-level latency
- Bit-for-bit reproducible runs
- Suitable for hard real-time systems
- Bare-metal compatible

---

## How to Use Automotive Components

### In Cargo.toml
```toml
[dependencies]
cu-automotive-payloads = { path = "../../components/payloads/cu_automotive_payloads" }
cu-can = { path = "../../components/tasks/cu_can" }
cu-isotp = { path = "../../components/tasks/cu_isotp" }
cu-someip = { path = "../../components/tasks/cu_someip" }
cu-uds = { path = "../../components/tasks/cu_uds" }
```

### In Copperconfig.ron
```ron
(
    tasks: [
        // CAN source
        (id: "can_rx", type: "cu_can::CanSource", config: {"interface": "can0"}),
        
        // CAN filter
        (id: "can_filter", type: "cu_can::CanFilter", config: {"accept_id": 0x7E8, "accept_mask": 0x7FF}),
        
        // ISO-TP codec
        (id: "isotp", type: "cu_isotp::IsotpCodec", config: {
            "tx_id": 0x7E0, "rx_id": 0x7E8,
            "n_bs_timeout_ms": 1000, "n_cr_timeout_ms": 1000,
        }),
        
        // UDS server
        (id: "uds_server", type: "cu_uds::UdsServer", config: {
            "session_timeout_ms": 5000,
            "max_security_attempts": 3,
        }),
        
        // SOME/IP router
        (id: "someip_router", type: "cu_someip::SomeIpRouter"),
    ],
    cnx: [
        (src: "can_rx", dst: "can_filter", msg: "cu_automotive_payloads::can::CanFrame"),
        (src: "can_filter", dst: "isotp", msg: "cu_automotive_payloads::can::CanFrame"),
        (src: "isotp", dst: "uds_server", msg: "cu_automotive_payloads::isotp::IsotpPdu"),
    ],
)
```

---

Generated: 2025-03-10
