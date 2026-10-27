//! Explicitly opted-in, debug-only native test host isolation.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
#[cfg(not(feature = "desktop-e2e-os"))]
use tauri::Manager;

#[cfg(all(target_os = "macos", not(feature = "desktop-e2e-os")))]
fn prevent_activation() -> Result<()> {
    use objc2::runtime::{AnyObject, Bool, Imp, Sel};
    unsafe extern "C-unwind" fn ignore_activation(_: *mut AnyObject, _: Sel) {}
    unsafe extern "C-unwind" fn ignore_activation_with_flag(_: *mut AnyObject, _: Sel, _: Bool) {}
    // These method tables belong only to this test process. Tao activates NSApplication
    // at launch even for a non-key window, so block that path before creating the app.
    // Signatures exactly match NSApplication's void activation methods on macOS.
    let class = objc2::class!(NSApplication);
    unsafe {
        class.instance_method(objc2::sel!(activateIgnoringOtherApps:))
            .context("missing NSApplication activation method")?
            .set_implementation(std::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel, Bool), Imp
            >(ignore_activation_with_flag));
        if let Some(method) = class.instance_method(objc2::sel!(activate)) {
            method.set_implementation(std::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel), Imp
            >(ignore_activation));
        }
    }
    Ok(())
}

#[cfg(feature = "desktop-e2e-os")]
fn require_virtual_mac() -> Result<String> {
    let output = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.model"])
        .output()
        .context("OS observations require a macOS virtual machine")?;
    let model = String::from_utf8(output.stdout)?.trim().to_owned();
    if !output.status.success() || !model.starts_with("VirtualMac") {
        bail!("OS observation build refuses a physical Mac: {model}");
    }
    Ok(model)
}

/// Keep WebKit rendering without creating a keyboard or pointer target on the user's desktop.
#[cfg(not(feature = "desktop-e2e-os"))]
pub fn isolate_window(app: &tauri::App) -> Result<()> {
    let window = app.get_webview_window("main").context("missing test window")?;
    window.set_focusable(false)?;
    window.set_ignore_cursor_events(true)?;
    #[cfg(target_os = "macos")]
    {
        // A hidden WKWebView suspends requestAnimationFrame. A transparent, non-key,
        // click-through window stays renderable while remaining inaccessible to user input.
        let native = window.ns_window()?;
        unsafe {
            let native = &*native.cast::<objc2::runtime::AnyObject>();
            let _: () = objc2::msg_send![native, setAlphaValue: 0.0_f64];
            let _: () = objc2::msg_send![native, setIgnoresMouseEvents: objc2::runtime::Bool::YES];
        }
        window.set_visible_on_all_workspaces(true)?;
        window.set_always_on_top(true)?;
    }
    Ok(())
}

#[cfg(not(feature = "desktop-e2e-os"))]
pub fn start_rendering(app: &tauri::AppHandle) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let window = app.get_webview_window("main").context("missing test window")?;
        let native = window.ns_window()?;
        // Wait until Tao has finished its launch-time window activation loop.
        // show() would call makeKeyAndOrderFront, so order without activation.
        unsafe {
            let native = &*native.cast::<objc2::runtime::AnyObject>();
            let _: () = objc2::msg_send![native, orderFrontRegardless];
        }
    }
    Ok(())
}

#[cfg(not(feature = "desktop-e2e-os"))]
pub fn record_input_isolation(app: &tauri::AppHandle) -> Result<()> {
    let window = app.get_webview_window("main").context("missing test window")?;
    let mut value = serde_json::json!({"focused": window.is_focused()?, "automatic": true});
    #[cfg(target_os = "macos")]
    unsafe {
        let native = window.ns_window()?;
        let native = &*native.cast::<objc2::runtime::AnyObject>();
        let alpha: f64 = objc2::msg_send![native, alphaValue];
        let ignored: objc2::runtime::Bool = objc2::msg_send![native, ignoresMouseEvents];
        let application: *mut objc2::runtime::AnyObject = objc2::msg_send![objc2::class!(NSApplication), sharedApplication];
        let active: objc2::runtime::Bool = objc2::msg_send![application, isActive];
        value["opacity"] = serde_json::json!(alpha);
        value["ignoresMouseEvents"] = serde_json::json!(ignored.as_bool());
        value["applicationActive"] = serde_json::json!(active.as_bool());
    }
    let root = PathBuf::from(std::env::var_os("MSV_DESKTOP_E2E_ROOT").context("missing test root")?);
    std::fs::write(root.join("output/input-isolation.json"), serde_json::to_vec_pretty(&value)?)?;
    if value["focused"] == true || value["applicationActive"] == true || value["opacity"] != 0.0 || value["ignoresMouseEvents"] != true {
        bail!("automatic test host must not receive desktop input: {value}");
    }
    Ok(())
}

pub fn prepare() -> Result<()> {
    if std::env::var("MSV_DESKTOP_E2E").as_deref() != Ok("1") {
        bail!("desktop-e2e requires MSV_DESKTOP_E2E=1");
    }
    if std::env::var("MSV_E2E_PROBE").as_deref() == Ok("startup") {
        bail!("deliberate native startup failure for cleanup verification");
    }
    // Check before creating NSApplication or a window. This build may receive OS
    // input only inside a guest, never on the user's physical desktop.
    #[cfg(feature = "desktop-e2e-os")]
    let virtual_model = require_virtual_mac()?;
    let root = PathBuf::from(std::env::var_os("MSV_DESKTOP_E2E_ROOT").context("missing test root")?);
    let canonical = root.canonicalize().context("test root must already exist")?;
    let temporary = std::env::temp_dir().canonicalize()?;
    if root != canonical
        || canonical.parent() != Some(temporary.as_path())
        || !canonical.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("msv-desktop-ux-"))
        || !canonical.join("fixture-owner.json").is_file()
    {
        bail!("desktop-e2e requires a fresh task-owned canonical temporary root");
    }
    for name in ["config", "data", "logs", "output"] {
        let path = canonical.join(name);
        if path.canonicalize()? != path {
            bail!("desktop-e2e directory must not contain symlinks: {}", path.display());
        }
    }
    let config = canonical.join("config/config.toml");
    if config.canonicalize()? != config {
        bail!("desktop-e2e config must stay inside the test root");
    }
    let port: u16 = std::env::var("TAURI_WEBDRIVER_PORT")?.parse()?;
    if port == 0 {
        bail!("desktop-e2e requires an allocated WebDriver port");
    }
    // build_runtime owns config/, data/ and logs/ beneath this root. The
    // canonical desktop devBridge discovery path is never used by this host.
    std::env::set_var("MSV_DAEMON_TEST_DATA_DIR", &canonical);
    std::env::set_var("MSV_DESKTOP_EMBEDDED_DEV", "1");
    std::fs::write(canonical.join("app.pid"), std::process::id().to_string())?;
    #[cfg(feature = "desktop-e2e-os")]
    std::fs::write(canonical.join("output/os-isolation.json"), serde_json::to_vec_pretty(
        &serde_json::json!({"model": virtual_model, "pid": std::process::id(), "virtualMachine": true})
    )?)?;
    #[cfg(all(target_os = "macos", not(feature = "desktop-e2e-os")))]
    prevent_activation()?;
    Ok(())
}

/// Authenticated HTTP parity lane sharing the same isolated facade as invoke.
pub async fn start_http(router: axum::Router) -> Result<()> {
    let root = PathBuf::from(std::env::var_os("MSV_DESKTOP_E2E_ROOT").context("missing test root")?);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    std::fs::write(
        root.join("output/native-http.json"),
        format!("{{\"port\":{port},\"baseUrl\":\"http://127.0.0.1:{port}\"}}"),
    )?;
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            tracing::error!(%error, "native test HTTP lane failed");
        }
    });
    Ok(())
}

#[derive(Default)]
struct FixturePartitions(std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>);
impl my_supervisor_application::PagePartitionFailureSource for FixturePartitions {
    fn failed_partitions(&self, family: &str) -> Vec<String> {
        self.0.lock().ok().and_then(|state| state.get(family).cloned()).unwrap_or_default()
    }
}

/// Feed the existing facade fault boundary without replacing IPC responses.
pub fn install_page_fixtures(facade: std::sync::Arc<my_supervisor_application::OperationsFacade>) {
    let state = std::sync::Arc::new(FixturePartitions::default());
    facade.set_page_partition_failure_source(state.clone());
    let root = PathBuf::from(std::env::var_os("MSV_DESKTOP_E2E_ROOT").expect("validated test root"));
    tokio::spawn(async move {
        let mut revision = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let path = root.join("output/partition-fixture.json");
            let Ok(metadata) = tokio::fs::metadata(&path).await else { continue };
            if metadata.len() > 65536 { continue; }
            let Ok(bytes) = tokio::fs::read(path).await else { continue };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
            let Some(next) = value["revision"].as_u64() else { continue };
            if next == revision { continue; }
            let mut partitions = std::collections::HashMap::new();
            for family in ["processes", "jobs"] {
                let names = value["failures"][family].as_array().map(|items| {
                    items.iter().take(64).filter_map(|item| item.as_str())
                        .filter(|item| item.len() < 256).map(str::to_owned).collect()
                }).unwrap_or_default();
                partitions.insert(family.to_string(), names);
            }
            if let Ok(mut current) = state.0.lock() { *current = partitions; }
            revision = next;
            let _ = tokio::fs::write(root.join("output/partition-applied.json"), next.to_string()).await;
        }
    });
}
