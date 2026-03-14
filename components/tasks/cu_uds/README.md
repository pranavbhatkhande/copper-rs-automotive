# cu_uds

ISO 14229 Unified Diagnostic Services (UDS) server and client tasks for the Copper runtime.

## Tasks

| Task | Trait | Description |
|------|-------|-------------|
| `UdsServer` | `CuTask` | Full UDS diagnostic server with session, security, and DID support |
| `UdsClient` | `CuTask` | UDS tester/client with request queue and timeout management |

### UDS Server Features

- **Diagnostic Session Control** (0x10) — Default, Programming, Extended sessions
- **ECU Reset** (0x11) — Hard/soft/key-off reset
- **Security Access** (0x27) — Seed/key authentication with attempt limiting and lockout
- **Tester Present** (0x3E) — Keep-alive with suppress-positive-response support
- **Read Data By Identifier** (0x22) — DID read with built-in VIN (0xF190)
- **Write Data By Identifier** (0x2E) — DID write (extended session required)
- **Routine Control** (0x31) — Start/stop/request results
- **S3 Session Timeout** — Auto-revert to Default session after inactivity (default 5000ms)
- **P2/P2\* Timing** — Configurable server response timing parameters

### UDS Client Features

- Request queue (up to 8 pending requests)
- P2 timeout management — clears awaiting state on server non-response (default 1000ms)
- Automatic response matching

## Configuration (RON)

### Server
```ron
(
    id: "uds_server",
    type: "cu_uds::UdsServer",
    config: {
        "session_timeout_ms": 5000,
        "p2_server_ms": 50,
        "p2_star_server_ms": 5000,
        "max_security_attempts": 3,
        "security_lockout_ms": 10000,
    },
),
```

### Client
```ron
(
    id: "uds_client",
    type: "cu_uds::UdsClient",
    config: {
        "p2_timeout_ms": 1000,
    },
),
```

## I/O Types

Both server and client use `IsotpPdu` as input and output, designed to connect directly to the ISO-TP codec layer.
