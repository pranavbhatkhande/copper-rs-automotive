# cu_isotp

ISO 15765-2 (ISO-TP) transport layer codec for the Copper runtime.

Implements segmentation and reassembly of multi-frame ISO-TP messages over CAN. Handles Single Frame, First Frame, Consecutive Frame, and Flow Control frame types with full state machine management.

## Task

| Task | Trait | Description |
|------|-------|-------------|
| `IsotpCodec` | `CuTask` | Bidirectional ISO-TP codec — reassembles RX, segments TX |

### I/O Types

- **Input:** `(CanFrame, IsotpPdu)` — CAN frame from network + ISO-TP PDU from upper layer
- **Output:** `(CanFrame, IsotpPdu)` — CAN frame to transmit + reassembled ISO-TP PDU to upper layer

## Configuration (RON)

```ron
(
    id: "isotp",
    type: "cu_isotp::IsotpCodec",
    config: {
        "tx_id": 0x641,
        "rx_id": 0x642,
        "block_size": 0,
        "st_min_ms": 10,
        "n_bs_timeout_ms": 1000,
        "n_cr_timeout_ms": 1000,
        "n_wft_max": 10,
    },
),
```

### Config Parameters

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `tx_id` | i64 | 0x7E0 | CAN ID for transmitted frames |
| `rx_id` | i64 | 0x7E8 | CAN ID filter for received frames |
| `block_size` | i64 | 0 | Flow Control block size (0 = no limit) |
| `st_min_ms` | i64 | 10 | Separation time in our Flow Control replies (ms) |
| `n_bs_timeout_ms` | i64 | 1000 | Timeout waiting for Flow Control after First Frame (ms) |
| `n_cr_timeout_ms` | i64 | 1000 | Timeout waiting for Consecutive Frame during reassembly (ms) |
| `n_wft_max` | i64 | 10 | Maximum Wait Flow Control frames before aborting TX |

## Protocol Features

- Single Frame (≤7 bytes) — immediate transfer
- First Frame + Consecutive Frames — segmented transfer for messages up to 4095 bytes
- Flow Control — block size and separation time management
- **STmin enforcement** — Parses STmin from received FC per ISO 15765-2 encoding (ms and 100μs ranges)
- **Transport timeouts** — N_Bs (FC wait) and N_Cr (CF wait) abort stalled transfers
- **Wait FC handling** — fs=1 Flow Control resets wait timer, aborts after N_WFTmax
- Normal and Extended addressing modes

## Determinism

All timers use `ctx.now()` (CuTime), making behavior fully deterministic under Copper's mock clock during replay and simulation.
