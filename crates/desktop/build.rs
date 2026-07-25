use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DETACHED_HELPERS: [&str; 2] = ["msv-log-proxy", "msv-group-reaper"];

fn main() {
    prepare_release_sidecars();
    tauri_build::build();
}

/// Tauri expects sidecars next to `tauri.conf.json`, suffixed with the exact
/// target triple.  The official server-release command builds the same helper
/// artifacts first; this build step copies and verifies those exact artifacts
/// instead of compiling a second, potentially mismatched helper profile.
fn prepare_release_sidecars() {
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");
    println!("cargo:rerun-if-env-changed=HOST");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=PROFILE");

    let target = required_env("TARGET");
    let profile = required_env("PROFILE");
    let helper_artifact_dir = cargo_artifact_dir(&target, &required_env("HOST"), &profile);
    let desktop_dir = PathBuf::from(required_env("CARGO_MANIFEST_DIR"));
    let sidecar_dir = desktop_dir.join("binaries");
    fs::create_dir_all(&sidecar_dir).expect("creating the Tauri sidecar directory");

    for helper_name in DETACHED_HELPERS {
        let source = helper_artifact_dir.join(helper_name);
        println!("cargo:rerun-if-changed={}", source.display());
        let sidecar = sidecar_dir.join(format!("{helper_name}-{target}"));
        if source.exists() {
            validate_executable(&source, helper_name);
            fs::copy(&source, &sidecar).unwrap_or_else(|error| {
                panic!(
                    "copying {helper_name} from {} to {} failed: {error}",
                    source.display(),
                    sidecar.display()
                )
            });
        } else if profile == "release" {
            validate_executable(&source, helper_name);
        } else {
            write_development_placeholder(&sidecar, helper_name);
        }
        validate_executable(&sidecar, helper_name);
    }
}

/// `cargo check` and a first `cargo tauri dev` do not compile dependency bin
/// targets.  Keep Tauri's config validation deterministic in that case, while
/// ensuring an attempted detached launch fails with an explicit instruction.
/// Release builds never use this path: they require the real same-profile
/// helper artifact above.
fn write_development_placeholder(path: &Path, helper_name: &str) {
    fs::write(
        path,
        format!(
            "#!/bin/sh\necho '{helper_name} is unavailable; run cargo build -p my-supervisor-platform-macos before starting detached processes' >&2\nexit 127\n"
        ),
    )
    .unwrap_or_else(|error| panic!("writing development placeholder {} failed: {error}", path.display()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|error| panic!("marking development placeholder {} executable failed: {error}", path.display()));
    }
}

fn cargo_artifact_dir(target: &str, host: &str, profile: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(required_env("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("desktop crate is nested under the workspace root");
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));
    let target_dir = if target == host {
        target_dir
    } else {
        target_dir.join(target)
    };
    target_dir.join(profile)
}

fn validate_executable(path: &Path, helper_name: &str) {
    let metadata = fs::metadata(path).unwrap_or_else(|error| {
        panic!(
            "required release helper {helper_name} is missing at {}: {error}. Run `cargo build --release -p my-supervisor-app-daemon -p my-supervisor-app-cli -p my-supervisor-platform-macos` before `cargo tauri build`.",
            path.display()
        )
    });
    if !metadata.is_file() {
        panic!("required release helper {helper_name} at {} is not a regular file", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if metadata.permissions().mode() & 0o111 == 0 {
            panic!("required release helper {helper_name} at {} is not executable", path.display());
        }
    }
}

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("Cargo did not provide required build variable {name}"))
}
