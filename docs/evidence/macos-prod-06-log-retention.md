# macOS production log-retention — v2 evidence

## 경합 재현과 수정

- detached journal은 수정 전 exact 반복 8번째에 `102`개 행과 `high_watermark=101`의 불일치가 재현됐다. writer의 file-first/manifest-after commit 중 reader가 서로 다른 snapshot을 읽은 관찰 순서 경합이었다.
- durable journal은 release-quality workspace 실행에서 high-watermark `10001`인데 물리 행 `9999`인 실패가 관찰됐다. append가 Tokio file buffer `flush` 완료 전에 watermark를 공개한 reader visibility 경합이었다.
- `crates/platform/macos/src/lifecycle.rs` reader는 실제 관찰한 최대 sequence만 watermark로 반환하고 rollover 중 manifest를 bounded cooperative retry로 재확인한다. 중복 sequence는 한 번만 노출한다.
- `crates/infra/logging/src/lib.rs` writer는 complete JSONL 행마다 `flush().await` 뒤에만 watermark/ring/broadcast를 전진시킨다. 기존 64행 `sync_data` durability cadence와 public API는 유지한다.

## 수정 후 반복 검증

| 검증 | 반복 | 결과 | 상태 |
|---|---:|---|---|
| detached exact journal | 10 | 10/10 exit `0` | pass |
| current durable exact stress, 10 concurrent processes | 10 | 10/10 exit `0` | pass |
| `cargo test -p my-supervisor-infra-logging` | 1 | 2 unit + 3 integration pass | pass |
| `cargo test -p my-supervisor-platform-macos` | 1 | 12 unit, 7 detached E2E, 5 lifecycle E2E pass; interactive launchd 3개는 기존 조건으로 ignored | pass |
| `cargo test --workspace` | 2 | elevated loopback exit `0`, `0` | pass |
| fmt/check/clippy `-D warnings` | 1 | 모두 exit `0` | pass |

non-elevated workspace 시도는 CLI ephemeral daemon의 sandbox loopback `Operation not permitted` 때문에 실패했으며 product failure 근거로 사용하지 않는다.

## 남은 매트릭스

Direct sustained rollover/retention, SystemRegistered 외부 log ingestion, Job-run cleanup interruption, expired cursor의 REST/Tauri/CLI 전체 조합은 이번 수정 후 반복에서 모두 실행한 것이 아니다. 이 광범위 log-retention matrix는 `partial`로 유지한다.
