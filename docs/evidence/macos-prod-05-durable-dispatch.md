# Wave 5 — durable dispatch evidence

- Revision: `2ede9d6` plus the uncommitted U11→U12 working-tree changes.
- Scope: atomic occurrence/cursor claim, durable admission/queue/retry, and final attempt outcome handoff.
- Fixture status: `unrun`. No isolated macOS daemon fixture was executed in this batch; no temporary root, port, label, SQLite rows, or process identities were created.

## Automated verification

| Check | Observed result | Status |
| --- | --- | --- |
| `cargo check --workspace` | exit 0 after the final source change | pass |
| `cargo test -p my-supervisor-core -p my-supervisor-application -p my-supervisor-infra-sqlite` | exit 0 | pass |
| `cargo test -p my-supervisor-application` | exit 0 after final compensation-path change | pass |
| `cargo test --workspace` in sandbox | CLI ephemeral daemon loopback bind returned `Operation not permitted` | environment-limited |
| `cargo test --workspace` in approved elevated environment (2026-07-26 replacement) | 최종 재실행 exit `0`; launchd 3개는 별도 ignored로 분리 | pass |
| `cargo test -p my-supervisor-app-daemon --test subprocess_recovery_e2e sigkill_restart_replays_terminal_outbox_to_one_cli_json_line_and_converges -- --exact` (2026-07-26 replacement) | exit `0`; SIGKILL/replay 후 terminal event가 CLI JSONL에 정확히 한 번 나타나고 outbox/cleanup이 수렴 | pass |
| `cargo test -p my-supervisor-app-cli --test e2e` (2026-07-26 replacement) | exit `0`; 2개 E2E 통과, follow process/run 각각 10,001행의 순증가·유일 cursor를 검증 | pass |

## Required macOS scenario matrix

| Scenario | Expected evidence | Observed | Status |
| --- | --- | --- | --- |
| claim before/after cursor commit crash seam | one logical occurrence and monotonic cursor | not executed | unrun |
| restart queued/retry occurrence | same occurrence identity and stored absolute retry time | not executed | unrun |
| `skip`, `run_once`, bounded `catch_up` downtime | original scheduled time and policy bound retained | not executed | unrun |
| one-shot and reused job name | no rearm; old `JobId` cannot attach to replacement | not executed | unrun |
| admission overflow/cancellation/final dependency | no pre-admission child; cancellation not retried; dependency once after final outcome | not executed | unrun |

## Cleanup and follow-up

- No fixture resources require cleanup because the semantic macOS fixture was not run.
- U13 starts from the stable occurrence/run identity and final state persisted by this batch; it owns segmented log journals, manifests, high-watermarks, retention, and related cursor transport work.
- The U09 macOS observation rows remain untouched. 과거 daemon CLI-follow timeout은 위 2026-07-26 재실행으로 대체됐지만, 이 문서의 durable-dispatch 전체 의미 매트릭스는 여전히 `unrun`이며 production-ready 제한이다. 대체 실행의 상세 근거는 `.agents/orchestration/20260726-macos-prod-release-readiness-01/phases/automated-gates/results.md`와 `.../interactive-local/results.md`다.
