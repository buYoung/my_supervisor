# macOS service owner — U18/U19 bounded evidence

## Observed on 2026-07-25

- Preparation: an isolated debug root under `/private/tmp/msv-u18.EnnjEd/data-root`, an empty config, and loopback ports `19876`–`19879`; the daemon was run with `MSV_DAEMON_TEST_DATA_DIR`, `MSV_DAEMON_TEST_CONFIG_PATH`, and `MSV_DAEMON_TEST_BIND_ADDR`.
- Owner/path result: the first daemon reached `listening`; `stat` observed root and `run/` at `0700`, and `run/owner.lock`, `run/control.token`, and `run/owner.json` at `0600`. The metadata contained endpoint, version, PID, native start identity, and credential generation, with no bearer secret.
- Contention result: a second daemon against the same root exited before binding with `daemon data root is already owned`.
- Stale-owner result: after the first isolated daemon was forcibly terminated with `SIGKILL`, a new daemon acquired the same root and listened on a new loopback port. The kernel-held `fcntl` lock therefore released on owner death before the replacement performed runtime assembly.
- Authentication result: missing and invalid bearer requests to `/api/v1/health` returned `401`; the native CLI loaded the user-only token from the isolated root, sent it as an `Authorization: Bearer` header, and successfully listed processes. The router middleware applies before REST handlers and WebSocket upgrade handlers; WebSocket sessions subscribe to credential-generation changes and close after rotation.

## U19 observed on 2026-07-25

- Token rotation preparation/action: an isolated daemon at `/private/tmp/msv-u19.UumbSH` bound `127.0.0.1:19891`; the initial native token successfully authenticated `/api/v1/health`, then `MSV_DAEMON_TEST_DATA_DIR=... cargo run -p my-supervisor-app-cli --bin msv -- --output json service rotate-token` completed with credential generation `3`.
- Token rotation result: the retained pre-rotation bearer returned `401`, the atomically replaced `run/control.token` returned `200`, and the same CLI command refreshed its native token after the authenticated response. No credential value was printed; the temporary root was removed after the observation. This directly closes the U18 runtime-observation gap for old-token rejection, new-token success, and native client recovery.
- Backup/upgrade/rollback preparation/action: against the same exclusive owner, `service backup`, `service upgrade`, and `service rollback` each returned success. Backup reported a verified manifest; upgrade and rollback persisted/returned `phase=committed`, active/rollback version `0.1.0`, and an isolated snapshot manifest. An authenticated health request after rollback returned `200`. The owner lock remained held by the daemon for all operations; runtime ownership files were excluded from backup copies.
- LaunchAgent lifecycle preparation/action: a second isolated root `/private/tmp/msv-u19-launch.7Cl2Dd`, debug label `com.my-supervisor.daemon.u19test`, debug plist override inside that root, and `127.0.0.1:19892` were used. `service install` returned `ready` with PID `46448`; repeat `service status` returned `ready`; `service stop` returned `stopped`; `service start` returned `ready` with PID `46587`; `service uninstall` returned `not_installed`. The plist was removed and the existing `logs/` and `run/` directories remained. Both isolated roots were removed after cleanup.

## Implementation boundary

- The supervisor lifecycle uses fixed production label `com.my-supervisor.daemon`, `gui/<uid>`, `RunAtLoad`, `ThrottleInterval=5`, stable root log paths, and an owner-private intentional-stop marker that suppresses `KeepAlive` until explicit start. `uninstall` removes only launchd registration/plist and never removes the data root.
- `service rotate-token`, `backup`, `upgrade`, and `rollback` are authenticated loopback RPCs. The owner serializes maintenance work, writes private metadata/journal files atomically, snapshots `data/`, `config/`, and `logs/`, and rejects invalid snapshot manifests before rollback.

## Unrun / deferred

- Login/reboot restoration and a browser-free installed desktop proxy remain unobserved. The existing desktop host continues to permit only explicit debug embedded ownership; the native CLI discovery/rebootstrap path is observed above. 배포된 이전 revision이 없으므로 legacy-root migration/managed-process plist rewrite는 최초 출시 gate가 아니다.
- Existing U09 macOS observations remain separate limitations. 과거 daemon CLI-follow loss/timeout과 workspace test-exit uncertainty는 2026-07-26 재실행으로 대체됐다.

## 2026-07-26 interactive launchd replacement

- `cargo test -p my-supervisor-platform-macos --test launchd_e2e launchd_registration_start_and_cleanup_leave_no_unit_or_plist -- --ignored --exact --nocapture`는 exit `0`이었다. `gui/501`, label `com.my-supervisor.e2e.9191dd90-edb1-47db-a17b-2fa3c6267f5a`, PID `99941`, plist mode `0644`를 관찰했고 종료 뒤 unit/plist/temp root 부재를 확인했다.
- 두 `launchd_config_saga_e2e` exact ignored scenario도 각각 exit `0`이었다. prepare-failure 보존 label/PID와 forward-recovery old/target label/PID를 관찰했고, 종료 뒤 unit/plist/candidate/temp root 및 incomplete journal 부재를 확인했다.
- 이 세 행은 과거 ignored 상태를 observed `pass`로 대체한다. login/reboot 복구와 installed desktop proxy는 계속 미관찰이다. 최초 출시에는 교체할 배포본이 없으므로 old→candidate binary switch와 legacy plist rewrite는 release gate에서 제외한다. 당시 전체 identity와 cleanup은 `.agents/orchestration/20260726-macos-prod-release-readiness-01/phases/interactive-local/results.md`에 있다.

## 2026-07-26 v2 recovery root identity

- `cargo test -p my-supervisor-app-daemon --test subprocess_recovery_e2e -- --nocapture`는 elevated loopback에서 exit `0`이었고 세 scenario의 exact root와 별도 UUID marker를 기록했다.
- 명시 cleanup과 panic-safe `Drop` fallback은 exact root 안의 non-symlink marker file이 같은 UUID일 때만 삭제한다. prefix가 비슷한 foreign root는 재귀 삭제 대상으로 선택하지 않는다.
- 이 결과는 로컬 recovery cleanup gate를 `pass`로 올린다. clean-user 설치와 login/reboot 복구는 별도 외부 gate로 남는다.
