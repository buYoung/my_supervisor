use crate::lifecycle::MacLifecycle;
use async_trait::async_trait;
use my_supervisor_core::ports::{
    CommandHelp, DeveloperTools, ExecutableCandidate, GuardError, RuntimeRelease,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

fn error(value: impl std::fmt::Display) -> GuardError {
    GuardError::Failed(value.to_string())
}
type HelpCache = Mutex<HashMap<String, (Instant, CommandHelp)>>;
static HELP_CACHE: OnceLock<HelpCache> = OnceLock::new();
type RuntimeReleaseCache = Mutex<HashMap<String, (Instant, Vec<RuntimeRelease>)>>;
static RELEASE_CACHE: OnceLock<RuntimeReleaseCache> = OnceLock::new();
async fn output(
    program: &str,
    args: &[String],
    cwd: Option<&str>,
    seconds: u64,
) -> Result<(bool, String), GuardError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .kill_on_drop(true)
        .env("NO_COLOR", "1")
        .env("PAGER", "cat")
        .env("MANPAGER", "cat")
        .env("CI", "1");
    if let Some(cwd) = cwd.filter(|s| !s.is_empty()) {
        command.current_dir(cwd);
    }
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().map_err(error)?;
    struct Group(Option<u32>);
    impl Drop for Group {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        }
    }
    let _group = Group(child.id());
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = async {
        let mut out = Vec::new();
        stdout.take(1_048_577).read_to_end(&mut out).await?;
        Ok::<_, std::io::Error>(out)
    };
    let read_error = async {
        let mut out = Vec::new();
        stderr.take(1_048_577).read_to_end(&mut out).await?;
        Ok::<_, std::io::Error>(out)
    };
    let (status, stdout, stderr) = tokio::time::timeout(Duration::from_secs(seconds), async {
        tokio::try_join!(child.wait(), read, read_error)
    })
    .await
    .map_err(|_| error("command exceeded its time limit"))?
    .map_err(error)?;
    if stdout.len() > 1_048_576 || stderr.len() > 1_048_576 {
        return Err(error("command output exceeded 2 MiB"));
    }
    Ok((
        status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        ),
    ))
}
async fn request(url: &str) -> Result<reqwest::Response, GuardError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("my-supervisor")
        .build()
        .map_err(error)?
        .get(url)
        .send()
        .await
        .map_err(error)?
        .error_for_status()
        .map_err(error)
}
async fn json(url: &str) -> Result<Value, GuardError> {
    request(url).await?.json().await.map_err(error)
}
fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

#[async_trait]
impl DeveloperTools for MacLifecycle {
    async fn executables(&self, name: &str) -> Result<Vec<ExecutableCandidate>, GuardError> {
        if name.is_empty() || name.contains('/') || name.contains('\0') {
            return Err(error("executable name must be a basename"));
        }
        let mut paths = Vec::new();
        let mut seen = HashSet::new();
        for folder in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            let path = folder.join(name);
            if path.is_file() && seen.insert(path.clone()) {
                paths.push(path);
            }
        }
        let mut result = Vec::new();
        for path in paths.into_iter().take(64) {
            let arg = if name == "go" {
                "version"
            } else if name == "java" {
                "-version"
            } else {
                "--version"
            };
            let version = output(&path.to_string_lossy(), &[arg.into()], None, 3)
                .await
                .map(|(_, v)| v.lines().next().unwrap_or("unknown").to_string())
                .unwrap_or_else(|_| "버전 확인 불가".into());
            result.push(ExecutableCandidate {
                name: name.into(),
                version,
                path: path.to_string_lossy().to_string(),
                is_default: result.is_empty(),
            });
        }
        Ok(result)
    }
    async fn command_help(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
    ) -> Result<CommandHelp, GuardError> {
        let resolved = if program.contains('/') {
            PathBuf::from(program)
        } else {
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|p| p.join(program))
                .find(|p| p.is_file())
                .unwrap_or_else(|| PathBuf::from(program))
        };
        let stamp = std::fs::metadata(resolved).and_then(|v| v.modified()).ok();
        let argument_stamps: Vec<_> = args
            .iter()
            .map(|arg| {
                std::fs::metadata(Path::new(cwd.unwrap_or(".")).join(arg))
                    .and_then(|v| v.modified())
                    .ok()
            })
            .collect();
        let key = format!("{program}:{args:?}:{cwd:?}:{stamp:?}:{argument_stamps:?}");
        if let Some((at, help)) = HELP_CACHE
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .get(&key)
        {
            if at.elapsed() < Duration::from_secs(3600) {
                let mut help = help.clone();
                help.cached = true;
                return Ok(help);
            }
        }
        let mut help_args = args.to_vec();
        help_args.push("--help".into());
        let result = output(program, &help_args, cwd, 4).await;
        let (ok, mut text) = result.unwrap_or_else(|e| (false, e.to_string()));
        if args.is_empty() && Path::new(program).file_name().is_some_and(|v| v == "java") {
            for extra in ["-help", "-X"] {
                if let Ok((_, value)) = output(program, &[extra.into()], cwd, 4).await {
                    text.push('\n');
                    text.push_str(&value);
                }
            }
        }
        let supported = ok
            || text.contains("Usage:")
            || text.contains("USAGE:")
            || text.contains("Options:")
            || text.contains("usage:");
        let help = CommandHelp {
            program: program.into(),
            text,
            supported,
            cached: false,
        };
        let mut cache = HELP_CACHE.get().unwrap().lock().unwrap();
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.insert(key, (Instant::now(), help.clone()));
        Ok(help)
    }
    async fn runtime_releases(
        &self,
        name: &str,
        all: bool,
    ) -> Result<Vec<RuntimeRelease>, GuardError> {
        let key = format!("{name}:{all}");
        if let Some((at, releases)) = RELEASE_CACHE
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .get(&key)
        {
            if at.elapsed() < Duration::from_secs(900) {
                return Ok(releases.clone());
            }
        }
        let mut result = Vec::new();
        match name {
            "node"=>{let data=json("https://nodejs.org/dist/index.json").await?;for release in data.as_array().into_iter().flatten(){if !all&&release["lts"]==false{continue;}let version=release["version"].as_str().unwrap_or("");let file=format!("node-{version}-darwin-{}.tar.gz",arch());if !release["files"].as_array().is_some_and(|files|files.iter().any(|v|v==&Value::String(format!("osx-{}-tar",arch())))){continue;}
                result.push(RuntimeRelease{version:version.into(),label:format!("Node.js {version}"),url:format!("https://nodejs.org/dist/{version}/{file}"),sha256:String::new()});if result.len()>=if all{100}else{20}{break;}}},
            "go"=>{let data=json("https://go.dev/dl/?mode=json&include=all").await?;for release in data.as_array().into_iter().flatten(){if !all&&release["stable"]!=true{continue;}for file in release["files"].as_array().into_iter().flatten(){if file["os"]=="darwin"&&file["arch"]==if arch()=="arm64"{"arm64"}else{"amd64"}&&file["kind"]=="archive"{let version=release["version"].as_str().unwrap_or("");result.push(RuntimeRelease{version:version.into(),label:version.into(),url:format!("https://go.dev/dl/{}",file["filename"].as_str().unwrap_or("")),sha256:file["sha256"].as_str().unwrap_or("").into()});}}if result.len()>=if all{100}else{20}{break;}}},
            "java"=>{let available=json("https://api.adoptium.net/v3/info/available_releases").await?;let majors=available[if all{"available_releases"}else{"available_lts_releases"}].as_array().cloned().unwrap_or_default();for major in majors.iter().rev().take(8){let url=format!("https://api.adoptium.net/v3/assets/latest/{}/hotspot?architecture={}&image_type=jdk&os=mac&vendor=eclipse",major.as_u64().unwrap_or(21),if arch()=="arm64"{"aarch64"}else{"x64"});for release in json(&url).await?.as_array().into_iter().flatten(){let version=release["version"]["semver"].as_str().unwrap_or("");let package=&release["binary"]["package"];result.push(RuntimeRelease{version:version.into(),label:format!("Eclipse Temurin {version}"),url:package["link"].as_str().unwrap_or("").into(),sha256:package["checksum"].as_str().unwrap_or("").into()});}}},
            "bun"=>{let data=json("https://api.github.com/repos/oven-sh/bun/releases?per_page=30").await?;let file=format!("bun-darwin-{}.zip",if arch()=="arm64"{"aarch64"}else{"x64"});for release in data.as_array().into_iter().flatten(){if !all&&release["prerelease"]==true{continue;}for asset in release["assets"].as_array().into_iter().flatten(){if asset["name"]==file{result.push(RuntimeRelease{version:release["tag_name"].as_str().unwrap_or("").into(),label:release["name"].as_str().unwrap_or("Bun").into(),url:asset["browser_download_url"].as_str().unwrap_or("").into(),sha256:asset["digest"].as_str().unwrap_or("").trim_start_matches("sha256:").into()});}}}},
            _=>return Err(GuardError::Unsupported("automatic installer supports node, go, java and bun; other executables can be selected by path"))
        }
        RELEASE_CACHE
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .insert(key, (Instant::now(), result.clone()));
        Ok(result)
    }
    async fn install_runtime(
        &self,
        name: &str,
        version: &str,
        directory: &str,
    ) -> Result<ExecutableCandidate, GuardError> {
        let destination = PathBuf::from(directory);
        if !destination.is_absolute() {
            return Err(error("installation path must be absolute"));
        }
        if destination.exists()
            && std::fs::read_dir(&destination)
                .map_err(error)?
                .next()
                .is_some()
        {
            return Err(error("installation directory must be empty"));
        }
        let release = self
            .runtime_releases(name, true)
            .await?
            .into_iter()
            .find(|r| r.version == version)
            .ok_or_else(|| error("select an available runtime version"))?;
        let mut checksum = release.sha256.clone();
        if name == "node" {
            let sums = request(&format!("https://nodejs.org/dist/{version}/SHASUMS256.txt"))
                .await?
                .text()
                .await
                .map_err(error)?;
            let filename = release.url.rsplit('/').next().unwrap_or("");
            checksum = sums
                .lines()
                .find_map(|line| {
                    let mut parts = line.split_whitespace();
                    let hash = parts.next()?;
                    let file = parts.next()?;
                    (file == filename).then(|| hash.to_string())
                })
                .ok_or_else(|| error("release checksum is missing"))?;
        }
        if checksum.len() != 64 || !checksum.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(error("release has no verifiable SHA-256 checksum"));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| error("invalid installation directory"))?;
        tokio::fs::create_dir_all(parent).await.map_err(error)?;
        let stage = parent.join(format!(".msv-install-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir(&stage).await.map_err(error)?;
        let result = async {
            let archive = stage.join(if name == "bun" {
                "runtime.zip"
            } else {
                "runtime.tar.gz"
            });
            let mut response = request(&release.url).await?;
            let mut file = tokio::fs::File::create(&archive).await.map_err(error)?;
            let mut total = 0u64;
            while let Some(chunk) = response.chunk().await.map_err(error)? {
                total += chunk.len() as u64;
                if total > 1_073_741_824 {
                    return Err(error("runtime archive is too large"));
                }
                file.write_all(&chunk).await.map_err(error)?;
            }
            file.flush().await.map_err(error)?;
            drop(file);
            let (_, actual) = output(
                "/usr/bin/shasum",
                &["-a".into(), "256".into(), archive.to_string_lossy().into()],
                None,
                120,
            )
            .await?;
            if actual.split_whitespace().next() != Some(checksum.as_str()) {
                return Err(error("runtime checksum does not match"));
            }
            let unpack = stage.join("unpacked");
            tokio::fs::create_dir(&unpack).await.map_err(error)?;
            let (ok, diagnostic) = if name == "bun" {
                output(
                    "/usr/bin/unzip",
                    &[
                        "-q".into(),
                        archive.to_string_lossy().into(),
                        "-d".into(),
                        unpack.to_string_lossy().into(),
                    ],
                    None,
                    120,
                )
                .await?
            } else {
                output(
                    "/usr/bin/tar",
                    &[
                        "-xzf".into(),
                        archive.to_string_lossy().into(),
                        "--strip-components=1".into(),
                        "-C".into(),
                        unpack.to_string_lossy().into(),
                    ],
                    None,
                    120,
                )
                .await?
            };
            if !ok {
                return Err(error(diagnostic));
            }
            if destination.exists() {
                tokio::fs::remove_dir(&destination).await.map_err(error)?;
            }
            tokio::fs::rename(&unpack, &destination)
                .await
                .map_err(error)?;
            let candidates = [
                destination.join(format!("bin/{name}")),
                destination.join("Contents/Home/bin/java"),
                destination.join(format!(
                    "bun-darwin-{}/bun",
                    if arch() == "arm64" { "aarch64" } else { "x64" }
                )),
            ];
            let path = candidates
                .into_iter()
                .find(|p| p.is_file())
                .ok_or_else(|| error("installed executable was not found"))?;
            Ok(ExecutableCandidate {
                name: name.into(),
                version: version.into(),
                path: path.to_string_lossy().into(),
                is_default: false,
            })
        }
        .await;
        let _ = tokio::fs::remove_dir_all(&stage).await;
        result
    }
    async fn read_text_file(&self, path: &str) -> Result<String, GuardError> {
        let metadata = tokio::fs::metadata(path).await.map_err(error)?;
        if metadata.len() > 2_097_152 {
            return Err(error("text file exceeds 2 MiB"));
        }
        tokio::fs::read_to_string(path).await.map_err(error)
    }
    async fn open_folder(&self, path: &str) -> Result<(), GuardError> {
        if !Path::new(path).is_dir() {
            return Err(error("directory does not exist"));
        }
        let (ok, diagnostics) = output("/usr/bin/open", &[path.into()], None, 10).await?;
        if ok {
            Ok(())
        } else {
            Err(error(diagnostics))
        }
    }
}
