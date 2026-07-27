# [feat] Add bounded cursor-safe log retention

## Work Type
feat

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, process and run output is appended to durable JSONL journals with monotonic numeric sequences, a 10,000-line memory ring, and broadcast follow — Evidence: `JournalState`, `append_to()`, and `tail_from()` in `crates/infra/logging/src/lib.rs`.
- [confirmed] Durable recovery reads each complete journal into memory and process journals have no size/age rotation contract — Evidence: `persist()` and `read_journal()` in `crates/infra/logging/src/lib.rs`.
- [confirmed] Job-run retention removes terminal run rows and their log files by count or age, but supervised process journals have no equivalent retention policy — Evidence: `LogRetention`, `enforce_log_retention()`, and `LogSink::remove_run`.
- [confirmed] Existing API and UI consumers resume logs by numeric sequence rather than file offset or segment identity — Evidence: log-tail DTOs, WebSocket mapping, and `LogsView.tsx`.
- [inferred] Replacing or truncating the active journal during rollover without durable high-watermark metadata can duplicate or skip entries after a crash — Confirm by: inject termination between segment seal, metadata update, and next-segment creation, then resume from the last acknowledged cursor.

## Desired Outcome (To-Be)
- Process and job-run journals rotate by explicit size and age bounds and retain only configured total history.
- Numeric cursor tail/follow remains strictly monotonic and gap-free across segment rollover and daemon restart while retained history exists.
- Tail reads select only segments that can contain the requested cursor and never load unrelated full history.
- Segment sealing, recovery, retention, and deletion are crash-safe and cannot remove an active segment.
- Retention ownership is explicit for Direct output, launchd stdout/stderr, detached log proxy output, and job-run journals.

## Scope
### In Scope
- Add segment metadata, atomic rollover, sequence high-watermarks, bounded reads, retention cleanup, and interrupted-cleanup recovery.
- Add compatible size/age/segment-count policies for process and job-run journals.
- Preserve the public numeric cursor and existing tail/follow contracts.
- Define ownership for externally written launchd files and internal proxy journals.
### Out of Scope
- [deferred] Metrics, operator events, alert rules, and notification delivery belong to `docs/briefs/2026-07-25-feat-macos-prod-07-telemetry-alerts.md`.
- [deferred] Log search and presentation belong to `docs/briefs/2026-07-25-feat-macos-prod-08-operator-ui.md`.
- [hard] Do not change authoritative process/job state from logging cleanup or I/O failure.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Start after stable instance identity in `crates/application/src/facade.rs` and durable occurrence identity in `crates/core/src/ports/repository.rs`.
- Keep logical sequence allocation independent of segment filenames and physical file offsets.
- Never delete the active segment, a segment still needed by an in-flight follow handoff, or a run journal before the existing deletion/retention saga permits it.
- Use bounded metadata and reads; a large retained history must not require loading every line at startup or per tail request.
- Preserve existing log cursor DTO shape unless an additive compatibility field is proven necessary.

## Related Files / Entry Points
- `crates/infra/logging/src/lib.rs` — start at journal append, recovery, sequence allocation, tail, follow, and removal.
- `crates/core/src/ports/log_sink.rs` — extend retention operations without exposing physical file layout.
- `crates/application/src/facade.rs` — propagate process/instance and run identities without changing supervision ownership.
- `crates/core/src/ports/repository.rs` — retain original occurrence/run identity and final outcome in log ownership.
- `crates/infra/http/src/ws.rs` — preserve reconnect high-watermark and numeric cursor semantics.
- `crates/desktop/ui/src/features/logs/LogsView.tsx` — verify existing cursor consumption remains compatible.
- `docs/evidence/macos-prod-06-log-retention.md` (proposed) — record rollover, crash recovery, retention, bounded-read, and expired-cursor evidence.

## Execution Plan
### Stage 1 — Define one segmented journal ownership model
- Starts when: Stable instance behavior exists at `crates/application/src/facade.rs` and durable occurrence delivery exists at `crates/core/src/ports/repository.rs`.
- Work: Specify segment naming, active/sealed lifecycle, sequence ranges, atomic metadata replacement, recovery ordering, and retention configuration while preserving numeric cursors.
- No-op when: Process and run journals already use one crash-safe segmented model with bounded metadata and compatible cursors.
- No-op handoff: Parent receives confirmed cursor-safe bounded storage at `crates/infra/logging/src/lib.rs`; telemetry work may start without storage changes.
- Deliverable: Segmented journal contract and compatibility defaults in `crates/infra/logging/src/lib.rs`.
- Verify: `bounded segment contract inspection`; Inputs: append, startup recovery, tail, follow, remove, launchd file, proxy file, and run-log paths; Expected: each writer has one owner, one active segment rule, and one cursor-preserving recovery route.
- Ends when:
  - [ ] Segment identity and numeric log sequence have distinct documented roles.
  - [ ] Existing unsegmented journals have an additive migration or compatible read route.
- Handoff: Stage 2 receives one physical storage and recovery contract.
- Replan when: A writer cannot participate in atomic rollover; retain its external file as an ingestion source and rotate only the supervisor-owned journal.

### Stage 2 — Implement bounded append, recovery, and tail
- Starts when: Stage 1 fixes the segment lifecycle and compatibility route.
- Work: Implement atomic sealing/creation, durable high-watermarks, startup repair, indexed segment selection, bounded tail, and follow handoff across rollover.
- Deliverable: Crash-safe bounded journal I/O in `crates/infra/logging/src/lib.rs`.
- Verify: `bounded rollover and crash-boundary inspection`; Inputs: a temporary journal with a small threshold, saved cursors before and after rollover, and termination injected at each metadata transition; Expected: strictly increasing unique sequences, exact retained-cursor resume, one active segment, and bounded files read.
- Ends when:
  - [ ] Restart repairs incomplete rollover without reusing or skipping a committed sequence.
  - [ ] Tail reads only segments whose recorded ranges can satisfy the request.
  - [ ] Follow crosses rollover once without duplicate subscriptions or missed committed entries.
- Handoff: Stage 3 receives reliable segment lifecycle and cursor continuity.
- Replan when: Metadata and journal durability cannot be ordered safely on the supported filesystem; replace the metadata design with a rebuildable append-only segment manifest while preserving the public cursor.

### Stage 3 — Enforce retention and deletion safely
- Starts when: Stage 2 can identify active, sealed, and recoverable segments.
- Work: Apply size, age, and segment-count limits; integrate job-run cleanup; recover interrupted cleanup; and surface explicit truncation boundaries for expired cursors.
- Deliverable: Production log-retention behavior at `crates/infra/logging/src/lib.rs`.
- Verify: `cargo test --workspace`; Inputs: existing suites after temporary process/run journal inspection records size, age, segment, interrupted-cleanup, bounded-read, and expired-cursor outcomes; Expected: exit code `0` and every Stage 3 end condition is recorded at `docs/evidence/macos-prod-06-log-retention.md`.
- Ends when:
  - [ ] Process and run logs remain within configured limits after sustained output and restart.
  - [ ] Job deletion, run retention, and segment cleanup are idempotent and converge after failure.
  - [ ] Disk or cleanup failure is diagnosable without changing authoritative process/job state.
- Handoff: `docs/briefs/2026-07-25-feat-macos-prod-07-telemetry-alerts.md` and the parent receive bounded cursor-safe storage at `crates/infra/logging/src/lib.rs`.
- Replan when: Existing clients cannot distinguish an expired cursor; add an additive earliest-retained response field and update every transport before claiming gap-free retained-history behavior.

## Side Effect Checkpoints
- [ ] Existing numeric cursor clients reconnect across rollover without API replacement.
- [ ] Journal recovery does not delay startup in proportion to total retained line count.
- [ ] Job deletion and retention sagas never recreate sealed or deleted run logs.
- [ ] Launchd and log-proxy writers do not race the supervisor for physical rotation ownership.
- [ ] Cleanup cannot delete definitions, run rows, active handles, or active journal segments.

## Acceptance Criteria
- [ ] Process and run log storage returns to configured size/age/segment bounds after sustained output and restart.
- [ ] Tail/follow across rollover and recovery returns strictly increasing unique sequences for all retained entries.
- [ ] Large-history tail and startup recovery read bounded metadata and only relevant segments.
- [ ] Interrupted rollover and cleanup converge to one active segment and correct retained ranges.
- [ ] Expired cursors produce an explicit retention-boundary result rather than silent gaps.

## Open Questions
- None — the public numeric cursor is preserved and physical segmentation remains an internal logging concern.
