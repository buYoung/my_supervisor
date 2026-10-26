"""Official runtime catalogs and explicit, isolated installs for the local preview."""

import concurrent.futures
import hashlib
import json
import os
import platform
import re
import shutil
import tarfile
import tempfile
import threading
import time
import urllib.parse
import urllib.request
import uuid
from pathlib import Path

DATA_ROOT = Path.home() / "Library/Application Support/my-supervisor-preview"
REGISTRY = DATA_ROOT / "installed-runtimes.json"
CATALOGS = {}
JOBS = {}
LOCK = threading.RLock()
LABELS = {"node": "Node.js", "go": "Go", "python": "Python", "java": "Java · Eclipse Temurin JDK"}


def fetch_bytes(url, limit=12 * 1024 * 1024):
    request = urllib.request.Request(url, headers={"User-Agent": "my-supervisor-preview/1.0", "Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=20) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError("배포 목록이 허용된 크기를 초과했습니다.")
    return data


def fetch_json(url):
    return json.loads(fetch_bytes(url))


def artifact(family, version, url, size=0, checksum=None, checksum_url=None, lts=False):
    identity = hashlib.sha256(f"{family}:{version}:{url}".encode()).hexdigest()[:24]
    return {"id": identity, "family": family, "version": version, "url": url, "size": size,
            "checksum": checksum, "checksumUrl": checksum_url, "lts": bool(lts), "label": LABELS[family]}


def runtime_catalog(family):
    if family not in LABELS:
        raise ValueError("설치를 지원하는 실행자는 Node.js, Go, Python, Java입니다.")
    with LOCK:
        cached = CATALOGS.get(family)
        if cached and time.monotonic() - cached[0] < 600:
            return cached[1]
    system = {"Darwin": "darwin", "Linux": "linux"}.get(platform.system())
    machine = platform.machine().lower()
    arch = "arm64" if machine in ("arm64", "aarch64") else "amd64"
    if system is None:
        raise ValueError("이 미리보기 설치기는 macOS와 Linux 아카이브를 지원합니다.")
    releases = []
    if family == "node":
        node_arch = "arm64" if arch == "arm64" else "x64"
        target = f"osx-{node_arch}-tar" if system == "darwin" else f"linux-{node_arch}"
        for item in fetch_json("https://nodejs.org/dist/index.json"):
            if target not in item.get("files", []):
                continue
            version = item["version"]
            base = f"https://nodejs.org/dist/{version}/"
            releases.append(artifact(family, version.lstrip("v"), base + f"node-{version}-{system}-{node_arch}.tar.gz",
                                     checksum_url=base + "SHASUMS256.txt", lts=item.get("lts")))
    elif family == "go":
        for item in fetch_json("https://go.dev/dl/?mode=json&include=all"):
            if not item.get("stable"):
                continue
            package = next((file for file in item["files"] if file["os"] == system and file["arch"] == arch and file["kind"] == "archive"), None)
            if package:
                releases.append(artifact(family, item["version"][2:], "https://go.dev/dl/" + package["filename"],
                                         package["size"], package["sha256"]))
    elif family == "python":
        release = fetch_json("https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest")
        assets = {item["name"]: item for item in release["assets"]}
        target = ("aarch64" if arch == "arm64" else "x86_64") + ("-apple-darwin" if system == "darwin" else "-unknown-linux-gnu")
        for name, item in assets.items():
            match = re.fullmatch(r"cpython-(\d+\.\d+\.\d+)\+[^/]+-" + re.escape(target) + r"-install_only\.tar\.gz", name)
            if not match:
                continue
            digest = item.get("digest") or ""
            checksum = digest[7:] if digest.startswith("sha256:") else None
            companion = assets.get(name + ".sha256")
            if checksum or companion:
                releases.append(artifact(family, match.group(1), item["browser_download_url"], item["size"], checksum,
                                         companion["browser_download_url"] if companion else None))
    else:
        info = fetch_json("https://api.adoptium.net/v3/info/available_releases")
        lts = info["available_lts_releases"]
        majors = sorted(set(lts[-4:] + [info["most_recent_feature_release"]]), reverse=True)
        java_arch = "aarch64" if arch == "arm64" else "x64"
        java_os = "mac" if system == "darwin" else "linux"

        def java_releases(major):
            query = urllib.parse.urlencode({"architecture": java_arch, "image_type": "jdk", "jvm_impl": "hotspot",
                                            "os": java_os, "page_size": 20, "project": "jdk"})
            values = []
            for release in fetch_json(f"https://api.adoptium.net/v3/assets/feature_releases/{major}/ga?{query}"):
                for binary in release.get("binaries", []):
                    package = binary["package"]
                    if package["name"].endswith(".tar.gz"):
                        version = re.sub(r"(\+\d+)\.0(?:\.LTS)?$", r"\1", release["version_data"]["semver"])
                        values.append(artifact(family, version, package["link"],
                                               package["size"], package["checksum"], lts=major in lts))
                        break
            return values

        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            for values in pool.map(java_releases, majors):
                releases.extend(values)
    releases.sort(key=lambda item: tuple(int(number) for number in re.findall(r"\d+", item["version"])), reverse=True)
    if not releases:
        raise ValueError("현재 플랫폼에 맞는 검증 가능한 배포 파일을 찾지 못했습니다.")
    result = {"family": family, "label": LABELS[family], "platform": f"{platform.system()} · {platform.machine()}",
              "defaultRoot": str(DATA_ROOT / "runtimes" / family), "releases": releases}
    with LOCK:
        CATALOGS[family] = (time.monotonic(), result)
    return result


def installed_runtimes():
    try:
        values = json.loads(REGISTRY.read_text())
        return [value for value in values if isinstance(value, dict) and os.path.isfile(value.get("path", "")) and os.access(value["path"], os.X_OK)]
    except (OSError, ValueError, TypeError):
        return []


def public_catalog(family):
    catalog = runtime_catalog(family)
    return {**catalog, "releases": [{key: value for key, value in item.items() if key not in ("url", "checksum", "checksumUrl")}
                                    for item in catalog["releases"]]}


def job_status(job_id):
    with LOCK:
        if job_id not in JOBS:
            raise ValueError("설치 작업을 찾을 수 없습니다.")
        return {key: value for key, value in JOBS[job_id].items() if key != "cancel"}


def cancel_install(job_id):
    with LOCK:
        if job_id not in JOBS:
            raise ValueError("설치 작업을 찾을 수 없습니다.")
        if JOBS[job_id]["phase"] not in ("complete", "failed", "cancelled"):
            JOBS[job_id]["cancel"] = True
    return job_status(job_id)


def start_install(family, release_id, destination):
    release = next((item for item in runtime_catalog(family)["releases"] if item["id"] == release_id), None)
    if release is None:
        raise ValueError("배포 목록을 새로 불러온 뒤 버전을 선택하세요.")
    path = Path(os.path.expanduser(destination))
    if not path.is_absolute() or path == Path(path.anchor) or path == Path.home():
        raise ValueError("새 실행자를 설치할 절대 경로를 지정하세요.")
    if path.exists() or path.is_symlink():
        raise ValueError("이미 존재하는 경로에는 설치하지 않습니다. 새 폴더 경로를 지정하세요.")
    job_id = uuid.uuid4().hex
    with LOCK:
        JOBS[job_id] = {"id": job_id, "phase": "queued", "downloaded": 0, "total": release["size"],
                        "destination": str(path), "version": release["version"], "family": family, "cancel": False}
    threading.Thread(target=install_worker, args=(job_id, release, path), daemon=True).start()
    return job_status(job_id)


def install_worker(job_id, release, destination):
    staging = None
    reserved = False

    def update(**values):
        with LOCK:
            JOBS[job_id].update(values)
            if JOBS[job_id]["cancel"]:
                raise InterruptedError("설치를 취소했습니다.")

    try:
        checksum = release["checksum"]
        if checksum is None:
            manifest = fetch_bytes(release["checksumUrl"], 4 * 1024 * 1024).decode()
            filename = urllib.parse.urlsplit(release["url"]).path.rsplit("/", 1)[-1]
            checksum = next((line.split()[0] for line in manifest.splitlines()
                             if re.match(r"^[a-fA-F0-9]{64}", line) and (filename in line or len(line.split()) == 1)), None)
        if checksum is None or not re.fullmatch(r"[a-fA-F0-9]{64}", checksum):
            raise ValueError("배포 파일의 SHA-256을 확인할 수 없습니다.")
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.mkdir()
        reserved = True
        staging = Path(tempfile.mkdtemp(prefix=".runtime-install-", dir=destination.parent))
        archive = staging / "download.tar.gz"
        update(phase="downloading")
        request = urllib.request.Request(release["url"], headers={"User-Agent": "my-supervisor-preview/1.0"})
        digest, downloaded = hashlib.sha256(), 0
        with urllib.request.urlopen(request, timeout=30) as response, archive.open("wb") as output:
            total = int(response.headers.get("Content-Length") or release["size"] or 0)
            update(total=total)
            while chunk := response.read(1024 * 1024):
                downloaded += len(chunk)
                if downloaded > 1024 * 1024 * 1024:
                    raise ValueError("배포 파일이 1 GiB 제한을 초과했습니다.")
                digest.update(chunk); output.write(chunk); update(downloaded=downloaded)
        update(phase="verifying")
        if digest.hexdigest().lower() != checksum.lower():
            raise ValueError("배포 파일의 SHA-256이 일치하지 않아 설치를 중단했습니다.")
        unpacked = staging / "content"
        unpacked.mkdir()
        update(phase="extracting")
        with tarfile.open(archive, "r:gz") as package:
            for member in package:
                update()
                package.extract(member, unpacked, filter="data")
        children = list(unpacked.iterdir())
        payload = children[0] if len(children) == 1 and children[0].is_dir() else unpacked
        executable = {"node": "node", "go": "go", "python": "python3", "java": "java"}[release["family"]]
        candidates = sorted(payload.glob(f"**/bin/{executable}"), key=lambda candidate: len(candidate.parts))
        binary = next((candidate for candidate in candidates if candidate.is_file() and candidate.resolve().is_relative_to(payload.resolve())), None)
        if binary is None:
            raise ValueError("압축 파일에서 실행 파일을 찾지 못했습니다.")
        relative = binary.relative_to(payload)
        update(phase="registering")
        os.replace(payload, destination)
        reserved = False
        installed = destination / relative
        entry = {"name": executable, "path": str(installed), "resolvedPath": str(installed.resolve()),
                 "pathIndex": None, "version": release["version"], "family": release["family"],
                 "label": LABELS[release["family"]], "source": "managed", "homePath": str(installed.parent.parent)}
        with LOCK:
            values = [value for value in installed_runtimes() if value["path"] != entry["path"]]
            values.append(entry)
            REGISTRY.parent.mkdir(parents=True, exist_ok=True)
            temporary = REGISTRY.with_suffix(".tmp")
            temporary.write_text(json.dumps(values, ensure_ascii=False, indent=2))
            temporary.replace(REGISTRY)
            JOBS[job_id].update(phase="complete", entry=entry)
    except InterruptedError as error:
        with LOCK:
            JOBS[job_id].update(phase="cancelled", error=str(error))
    except Exception as error:
        with LOCK:
            JOBS[job_id].update(phase="failed", error=str(error))
    finally:
        if staging:
            shutil.rmtree(staging, ignore_errors=True)
        if reserved:
            try:
                destination.rmdir()
            except OSError:
                pass
