# Wave 4 macOS Multi-instance Evidence

- Revision: U10 Rolling/public operations implementation (2026-07-25).
- Isolated fixture: planned `mktemp -d` root with `MSV_DAEMON_TEST_DATA_DIR`, `MSV_DAEMON_TEST_CONFIG_PATH`, `MSV_DAEMON_TEST_BIND_ADDR`, and `MSV_DAEMON_TEST_CONTROL_SOCKET`.
- Public driver: `cargo run -p my-supervisor-app-daemon --bin msv-daemon` and `cargo run -p my-supervisor-app-cli --bin msv -- --url http://127.0.0.1:<port>`.

| Scenario | Expected result | Observed | Status | Follow-up |
| --- | --- | --- | --- | --- |
| U09 start, daemon restart, one-child crash, scale up/down, stop/remove | Stable UUID/ordinal ownership with no sibling cleanup or duplicate child | Not run in this batch | unrun | Required production-ready gate |
| Scale and idempotent replay | Aggregate plus deterministic per-instance outcome; matching request ID replays durable result | Source/compile checked; real macOS observation not run | unrun | Run isolated daemon/CLI fixture |
| Rolling readiness success | Surge replacement becomes freshly ready before old slot drains; minimum healthy capacity remains | Source/compile checked; real macOS observation not run | unrun | Run isolated daemon/CLI fixture |
| Readiness timeout / child crash / cancellation | Old healthy capacity remains and durable compensation or fail-closed phase is recorded | Source/compile checked; real macOS observation not run | unrun | Run isolated daemon/CLI fixture |
| Daemon restart during rollout | Incomplete durable operation becomes explicit `failed_closed`; no implicit resume | Source/compile checked; real macOS observation not run | unrun | Run isolated daemon/CLI fixture |

Cleanup for every future fixture: terminate only the validated daemon process group and remove only its validated temporary root. The pre-existing daemon subprocess-recovery CLI-follow timeout is an independent limitation and is not attributed to Wave 4 rollout behavior.
