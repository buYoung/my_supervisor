# macOS runtime guards — Wave 2 evidence

## 2026-07-26 관찰

- 기준 source: `2ede9d6470200ae6877b9f830ca36a66d7a02f89` 위 dirty worktree. 자세한 identity와 정리는 `.agents/orchestration/20260726-macos-prod-release-readiness-01/phases/interactive-local/results.md`를 따른다.
- `cargo test -p my-supervisor-platform-macos --test lifecycle_e2e`는 exit `0`, 5 passed였다. caller cancellation, TERM을 무시하는 descendant 정리, restart cleanup, native generation이 stale child를 남기지 않는 기존 시나리오를 관찰했다.
- 테스트 소유 process group/temp root가 정리됐고 phase 소유 daemon/helper/process 잔여 증거는 없었다.

## 남은 필수 매트릭스

| 시나리오 | 상태 | 남은 작업 |
|---|---|---|
| caller cancellation, descendant cleanup, restart generation | pass | 최종 후보에서 guard source가 바뀌면 재실행 |
| process-group aggregate RSS memory ceiling | blocked | deterministic descendant RSS fixture로 action 1회와 group aggregate 측정 관찰 |
| file-watch burst/overflow/rescan | blocked | bounded overflow/rescan 및 stale generation 억제 관찰 |
| liveness/readiness delay·timeout 및 automatic action coalescing | blocked | 기존 public daemon/CLI로 재현 가능한 fixture를 마련해 전이와 cleanup 관찰 |

이 문서는 Wave 2 전체 `pass`가 아니다. 기존 하니스로 직접 관찰한 lifecycle 부분만 `pass`이며, 나머지 mandatory guard matrix는 `blocked`다.
