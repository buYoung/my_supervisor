use std::time::Duration;

use my_supervisor_core::domain::ProcessSpec;
use my_supervisor_core::ports::ProcessServiceRegistrar;
use my_supervisor_platform_macos::LaunchdAgentProcess;

fn temporary_directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("my-supervisor-launchd-e2e-{}", uuid::Uuid::new_v4()))
}

#[tokio::test]
#[ignore = "requires an interactive macOS GUI launchd session"]
async fn launchd_registration_start_and_cleanup_leave_no_unit_or_plist() {
    let directory = temporary_directory();
    tokio::fs::create_dir_all(&directory).await.unwrap();
    let label = format!("com.my-supervisor.e2e.{}", uuid::Uuid::new_v4());
    let mut spec = ProcessSpec::new("launchd-e2e", "/bin/sh");
    spec.args = vec!["-c".into(), "sleep 30".into()];
    let registrar = LaunchdAgentProcess::new(directory.clone());
    let home = std::env::var("HOME").expect("GUI-session HOME is available");
    let plist = std::path::PathBuf::from(home).join("Library/LaunchAgents").join(format!("{label}.plist"));
    // SAFETY: getuid is a pure libc query.
    let target = format!("gui/{}/{}", unsafe { libc::getuid() }, label);

    let result = async {
        registrar.register(&label, &spec).await?;
        assert!(plist.exists());
        registrar.start(&label).await?;
        for _ in 0..40 {
            if registrar.query_pid(&label).await?.is_some() {
                return Ok::<(), my_supervisor_core::ports::RegistrarError>(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err(my_supervisor_core::ports::RegistrarError::RegistrationFailed(
            "launchd unit did not publish a PID".into(),
        ))
    }
    .await;

    let cleanup = registrar.unregister(&label).await;
    let print_status = tokio::process::Command::new("launchctl")
        .args(["print", &target])
        .status()
        .await
        .expect("launchctl print starts");
    let plist_removed = !plist.exists();
    let _ = tokio::fs::remove_dir_all(&directory).await;

    result.expect("launchd registration/start succeeds");
    cleanup.expect("launchd cleanup succeeds");
    assert!(!print_status.success(), "removed launchd unit remains registered");
    assert!(plist_removed, "removed launchd plist remains on disk");
}
