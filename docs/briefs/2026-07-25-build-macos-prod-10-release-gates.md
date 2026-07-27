# [build] Gate the production macOS release

## Work Type
build

## Current State (As-Is)
- [confirmed] As of `a1c845c` on `main`, the Tauri bundle includes `msv-log-proxy` and `msv-group-reaper`, uses identifier `com.my-supervisor.desktop`, targets all bundle types, and has `csp: null` — Evidence: `crates/desktop/tauri.conf.json`.
- [confirmed] No signing identity, notarization workflow, updater, or explicit entitlement configuration is present in the repository — Evidence: `crates/desktop/tauri.conf.json` and scoped workflow/config search.
- [confirmed] Three launchd integration scenarios are ignored because they require an interactive macOS GUI launchd session — Evidence: `launchd_config_saga_e2e`, `launchd_e2e`, and current `cargo test --workspace` output.
- [confirmed] Baseline at the investigation point passes `cargo check --workspace`, escalated `cargo test --workspace`, `pnpm --dir crates/desktop/ui typecheck`, and `pnpm --dir crates/desktop/ui build`; three interactive launchd tests remain ignored — Evidence: recorded commands from this brief investigation.
- [confirmed] CI, API, architecture, development, and README files already have unrelated working-tree changes that must be preserved — Evidence: baseline `git status --short`.
- [inferred] A successful local compile does not prove the app bundle contains all daemon/CLI/helper artifacts at paths compatible with installation and upgrade — Confirm by: inspect an unsigned release candidate on a clean macOS user account before any publishing action.

## Desired Outcome (To-Be)
- The release candidate contains desktop, daemon, CLI, log proxy, and group reaper at stable resolvable paths compatible with installed-service upgrade and rollback.
- CSP, entitlements, hardened-runtime, signing, and notarization inputs are explicit and credentials remain outside the repository.
- CI and local release gates prove workspace build/test, UI typecheck/build, artifact inventory, migration compatibility, and package assembly.
- A documented production runbook records observed pass/fail for interactive login/reboot, launchd, rollout, scheduling downtime, log rollover, alerts, CLI, desktop, backup, upgrade, and rollback scenarios.
- No artifact is published, signed, notarized, or deployed without separate explicit approval and credentials.

## Scope
### In Scope
- Align Cargo and Tauri artifact manifests with installed-service layout.
- Replace placeholder security configuration with explicit least-privilege CSP and entitlement requirements.
- Add credential-free signing/notarization configuration inputs and failure messages.
- Define CI/local artifact inventory, migration, package, and production-readiness gates.
- Update architecture, development, API, and operational runbook documentation.
- Record automated and manual verification evidence without claiming unrun scenarios.
### Out of Scope
- [hard] Do not publish, upload, sign, notarize, deploy, or modify external release systems without separate approval and credentials.
- [hard] Do not add root/system-domain services or privileged helpers.
- [deferred] Linux and Windows packaging remain outside the macOS production release.
- [hard] Do not add new test files or test cases without separate user approval.

## Constraints
- Start after installed single-owner behavior is available at `crates/daemon/src/lib.rs` and the operator child has proven transport parity against it.
- Preserve all pre-existing working-tree changes and reconcile shared documentation/workflow files without overwriting them.
- Keep signing/notarization credentials external; repository files may define variable names and validation only.
- Treat interactive launchd/login/reboot/signing/notarization observations separately from automated results.
- A failed artifact, migration, rollback, security, or production scenario returns to the owning child brief; release readiness cannot waive it.
- Verify the local control-plane credential is generated at install time, stored `0600`, redacted, rotatable, required before routing, inaccessible to browser JavaScript/URLs, and absent from bundle, CI, logs, and evidence.
- `docs/PRODUCTION_READINESS.md` is an index, not a substitute for evidence; it must link every child-owned `docs/evidence/macos-prod-<wave>-*.md` record and mark any missing, failed, ignored, or unrun mandatory scenario as blocking.

## Related Files / Entry Points
- `crates/desktop/tauri.conf.json` — start with artifact inventory and security/release configuration.
- `Cargo.toml` — align workspace release artifacts and profiles.
- `crates/daemon/src/lib.rs` — consume the installed-service layout and compatibility contract.
- `.github/workflows/ci.yml` — add credential-free build and package gates while preserving existing edits.
- `docs/DEVELOPMENT.md` — document prerequisites and exact verification commands.
- `docs/ARCHITECTURE.md` — document one-owner packaged topology and rollback.
- `docs/API.md` — document compatibility/version behavior where public.
- `docs/PRODUCTION_READINESS.md` (proposed) — record scenario preconditions, artifact/version identity, expected outcome, observed result, status, cleanup, and owning follow-up.
- `docs/evidence/macos-prod-10-release-gates.md` (proposed) — record automated command results, artifact/security inventory, credential scan, and final manual-gate audit.

## Execution Plan
### Stage 1 — Define the release artifact and security contract
- Starts when: Installed single-owner behavior is available at `crates/daemon/src/lib.rs` and the complete installed-owner operator contract is available at `crates/desktop/ui/src/services/operations-client.ts`.
- Work: Enumerate desktop, daemon, CLI, helper, plist/template, resource, entitlement, CSP, version, and compatibility inputs; map each to package and installed locations.
- No-op when: The current release configuration already proves complete artifact inventory, least-privilege security settings, external credential inputs, and service-layout compatibility.
- No-op handoff: Parent receives confirmed release inputs at `crates/desktop/tauri.conf.json`; verification may proceed only after every installed artifact has one package source and destination.
- Deliverable: Production release contract in `crates/desktop/tauri.conf.json`.
- Verify: `bounded release-contract inspection`; Inputs: Tauri config, Cargo manifests, service installer layout, helper discovery, CSP, entitlements, signing/notarization variables, and version handshake; Expected: every artifact and security input has one owner, credentials are absent, and no placeholder such as null CSP remains.
- Ends when:
  - [ ] Package inventory includes desktop, daemon, CLI, both helpers, and required resources.
  - [ ] Entitlements and CSP are least-privilege and tied to actual runtime behavior.
  - [ ] Missing external release credentials fail explicitly only at the action that requires them.
- Handoff: Stage 2 receives a complete credential-free release configuration.
- Replan when: A bundled service binary cannot be upgraded independently and rollback safely; return to the service-owner brief and correct artifact layout before packaging.

### Stage 2 — Automate non-interactive production gates
- Starts when: Stage 1 fixes artifact paths, compatibility, and security inputs.
- Work: Align CI/local commands for Rust check/test, UI typecheck/build, bundle assembly, artifact inventory, schema migration/rollback preconditions, and unsigned package inspection.
- Deliverable: Repeatable credential-free release gates in `crates/desktop/tauri.conf.json` and `.github/workflows/ci.yml`.
- Verify: `cargo check --workspace && cargo test --workspace && pnpm --dir crates/desktop/ui typecheck && pnpm --dir crates/desktop/ui build`; Inputs: complete repository followed by unsigned bundle/artifact inventory inspection; Expected: commands exit `0`, package contains every declared artifact at the expected path, and only explicitly documented interactive/signing actions remain.
- Ends when:
  - [ ] Automated failures identify the owning child or artifact rather than being waived.
  - [ ] Existing UTC/single-instance data upgrades through every new migration and remains recoverable.
  - [ ] CI contains no credentials and does not perform external release actions.
- Handoff: Stage 3 receives an unsigned candidate and automated evidence.
- Replan when: A baseline command or artifact inspection fails; stop release evaluation, route the failure to its owning child, and rerun the complete gate after correction.

### Stage 3 — Record interactive production readiness
- Starts when: Stage 2 produces an unsigned candidate with all automated gates passing.
- Work: Audit and index every child evidence record, then execute any remaining isolated macOS scenarios for install/login/reboot/crash recovery, process guards/rollout, scheduler downtime/DST/retry/queue, logs, alerts, authenticated CLI/UI, backup, upgrade failure, rollback, and uninstall; document signing/notarization as approval-gated.
- Deliverable: Observed production-readiness evidence in `docs/PRODUCTION_READINESS.md` with one record per scenario containing date, source revision, artifact/version identity, isolated data/label, precondition, action, expected outcome, observed outcome, pass/fail/unrun status, cleanup result, and owning follow-up.
- Verify: `documented interactive macOS production inspection`; Inputs: unsigned candidate, every child evidence file, isolated user/data/labels, scenario preconditions, identities/outcomes, observed results, credential scan, and cleanup; Expected: every required scenario has dated `pass`, failed/ignored/unrun items block readiness, failures route to an owner, no credential leaks, and no external action occurs; record audit at `docs/evidence/macos-prod-10-release-gates.md` and index it in `docs/PRODUCTION_READINESS.md`.
- Ends when:
  - [ ] Login/reboot and crash recovery preserve one owner and stable state without duplication.
  - [ ] Process, scheduler, log, alert, client, backup, upgrade, rollback, and uninstall scenarios record observed outcomes.
  - [ ] Publishing, signing, notarization, and deployment remain explicitly pending separate approval.
- Handoff: Parent receives the complete evidence index at `docs/PRODUCTION_READINESS.md` and the package/security contract at `crates/desktop/tauri.conf.json` for global acceptance.
- Replan when: Any scenario fails, evidence lacks concrete identities/outcomes, or cleanup leaves an owner/artifact behind; return to the owning child and do not label the release production-ready.

## Side Effect Checkpoints
- [ ] Bundle artifact paths match service install/upgrade/rollback paths.
- [ ] CSP and entitlements grant only capabilities exercised by the application.
- [ ] Release configuration and logs contain no signing/notarization secret values.
- [ ] CI does not publish, sign, notarize, or deploy.
- [ ] Documentation distinguishes automated pass, manual observed pass, ignored test, and unrun approval-gated action.
- [ ] Existing edits in CI and documentation are merged rather than overwritten.

## Acceptance Criteria
- [ ] Unsigned release artifacts contain desktop, daemon, CLI, log proxy, and group reaper at resolvable stable paths.
- [ ] Security configuration is explicit, least-privilege, and contains no credentials.
- [ ] All existing automated verification commands exit `0` on the completed source and package inventory passes.
- [ ] Interactive macOS production scenarios have recorded observed results; ignored or unrun scenarios are not reported as passed.
- [ ] Failed gates route to an owning child and block production-ready status until corrected and reverified.
- [ ] No publish, signing, notarization, deployment, or external mutation occurs without separate explicit approval.

## Open Questions
- None — credential-free source/package readiness is in scope; outward release actions remain approval-gated.
