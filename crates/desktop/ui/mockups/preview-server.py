"""Serve the HTML study, local PATH discovery and explicitly requested runtime installs."""

import argparse
import concurrent.futures
import datetime
import functools
import http.server
import json
import os
import re
import subprocess
import threading
import time
import urllib.parse
from pathlib import Path
from runtime_installer import cancel_install, installed_runtimes, job_status, public_catalog, start_install
from runtime_help import executable_help
from application_help import application_help


ROOT = Path(__file__).resolve().parent
RULES = [
    (r"node(?:js)?", "node", "Node.js", ["--version"]),
    (r"python(?:\d+(?:\.\d+)*)?", "python", "Python", ["--version"]),
    (r"go", "go", "Go", ["version"]),
    (r"ruby(?:\d+(?:\.\d+)*)?", "ruby", "Ruby", ["--version"]),
    (r"java", "java", "Java", ["-version"]),
    (r"(?:bun|deno|php|dotnet|perl|lua|luajit|bash|zsh|sh|rustc|cargo|npm|pnpm|yarn)", None, None, ["--version"]),
]
VERSION_CACHE = {}
CACHE_LOCK = threading.Lock()


def runtime_rule(name):
    for pattern, family, label, arguments in RULES:
        if re.fullmatch(pattern, name):
            return family or name, label or name, arguments
    return name, name, None


def path_entries():
    entries, visited = [], set()
    for index, raw_directory in enumerate(os.environ.get("PATH", "").split(os.pathsep)):
        directory = os.path.abspath(os.path.expanduser(raw_directory or os.curdir))
        if directory in visited:
            continue
        visited.add(directory)
        try:
            with os.scandir(directory) as children:
                for child in children:
                    try:
                        if not child.is_file() or not os.access(child.path, os.X_OK):
                            continue
                        entries.append({"name": child.name, "path": child.path,
                                        "resolvedPath": os.path.realpath(child.path), "pathIndex": index})
                    except OSError:
                        continue
        except OSError:
            continue
    return sorted(entries, key=lambda entry: (entry["pathIndex"], entry["name"]))


def executable_version(entry):
    _, _, arguments = runtime_rule(entry["name"])
    if arguments is None or (entry["name"] == "java" and entry["resolvedPath"] == "/usr/bin/java"):
        return None
    try:
        stat = os.stat(entry["resolvedPath"])
        key = (entry["resolvedPath"], stat.st_mtime_ns, stat.st_size)
    except OSError:
        return None
    with CACHE_LOCK:
        cached = VERSION_CACHE.get(key)
        if cached and time.monotonic() - cached[0] < 60:
            return cached[1]
    version = None
    try:
        result = subprocess.run(
            [entry["path"], *arguments], capture_output=True, text=True, errors="replace",
            timeout=1.5, cwd="/private/tmp" if Path("/private/tmp").is_dir() else "/tmp",
            env={**os.environ, "GOTOOLCHAIN": "local", "NO_COLOR": "1"}, check=False,
        )
        if result.returncode == 0:
            output = re.sub(r"\x1b\[[0-9;]*m", "", result.stdout + "\n" + result.stderr)
            first_line = next((line.strip() for line in output.splitlines() if line.strip()), "")
            match = re.search(r"\d+(?:\.\d+){0,3}(?:[-+][\w.]+)?", first_line)
            if match:
                version = match.group(0)
    except (OSError, subprocess.TimeoutExpired):
        pass
    with CACHE_LOCK:
        if len(VERSION_CACHE) > 512:
            VERSION_CACHE.clear()
        VERSION_CACHE[key] = (time.monotonic(), version)
    return version


def discover_executables(command, cwd=""):
    name = os.path.basename(command)
    family, label, _ = runtime_rule(name)
    entries = [entry for entry in path_entries() if runtime_rule(entry["name"])[0] == family]
    default = next((entry["path"] for entry in entries if entry["name"] == name), None)
    # Resolve aliases once, while retaining every distinct PATH location in the list.
    unique = {}
    for entry in entries:
        unique.setdefault(entry["resolvedPath"], entry)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        versions = dict(zip(unique, pool.map(executable_version, unique.values())))
    for entry in entries:
        entry.update(version=versions[entry["resolvedPath"]], family=family, label=label, source="path")
        if family == "java" and entry["resolvedPath"] != "/usr/bin/java":
            entry["homePath"] = str(Path(entry["resolvedPath"]).parent.parent)
    known_paths = {entry["path"] for entry in entries}
    entries.extend(entry for entry in installed_runtimes() if entry["family"] == family and entry["path"] not in known_paths)
    current = None
    if "/" in command:
        absolute = os.path.abspath(os.path.join(cwd or os.getcwd(), os.path.expanduser(command)))
        current = next((entry for entry in entries if entry["path"] == absolute), None)
        if current is None and os.path.isfile(absolute) and os.access(absolute, os.X_OK):
            current = {"name": name, "path": absolute, "resolvedPath": os.path.realpath(absolute),
                       "pathIndex": None, "version": None, "family": family, "label": label, "source": "explicit"}
            if family == "java" and current["resolvedPath"] != "/usr/bin/java":
                current["homePath"] = str(Path(current["resolvedPath"]).parent.parent)
            entries.insert(0, current)
    else:
        current = next((entry for entry in entries if entry["path"] == default), None)
    return {"source": "local-path", "command": command, "family": family, "label": label,
            "scannedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "entries": entries, "defaultPath": default, "current": current,
            "help": executable_help(current) if current else None}


def list_directories(raw_path, nearest=False, show_hidden=False):
    if len(raw_path) > 4096 or "\0" in raw_path:
        raise ValueError("폴더 경로를 확인하세요.")
    directory = Path(raw_path).expanduser() if raw_path else Path.home()
    if not directory.is_absolute():
        raise ValueError("절대 경로를 입력하세요.")
    if nearest:
        while not directory.is_dir() and directory != directory.parent:
            directory = directory.parent
    directory = directory.resolve()
    if not directory.is_dir():
        raise ValueError("존재하는 폴더를 선택하세요.")
    folders = []
    with os.scandir(directory) as entries:
        for entry in entries:
            if not show_hidden and entry.name.startswith("."):
                continue
            try:
                if entry.is_dir():
                    folders.append({"name": entry.name, "path": str(directory / entry.name)})
            except OSError:
                continue
    folders.sort(key=lambda item: item["name"].casefold())
    return {"path": str(directory), "parent": str(directory.parent), "home": str(Path.home()),
            "folders": folders[:500], "total": len(folders)}


class PreviewHandler(http.server.SimpleHTTPRequestHandler):
    def is_local_request(self):
        origin = self.headers.get("Origin")
        allowed = {f"http://127.0.0.1:{self.server.server_port}", f"http://localhost:{self.server.server_port}"}
        return self.headers.get("Sec-Fetch-Site") != "cross-site" and (not origin or origin in allowed)

    def do_GET(self):
        url = urllib.parse.urlsplit(self.path)
        if not url.path.startswith("/__preview__/"):
            return super().do_GET()
        if not self.is_local_request():
            return self.send_json({"error": "Local preview requests only."}, 403)
        query = urllib.parse.parse_qs(url.query)
        if url.path == "/__preview__/directories":
            try:
                return self.send_json(list_directories(query.get("path", [""])[0],
                                      query.get("nearest", [""])[0] == "1", query.get("hidden", [""])[0] == "1"))
            except (ValueError, OSError) as error:
                return self.send_json({"error": str(error)}, 400)
        if url.path == "/__preview__/runtime-catalog":
            try:
                return self.send_json(public_catalog(query.get("family", ["node"])[0]))
            except Exception as error:
                return self.send_json({"error": str(error)}, 502)
        if url.path == "/__preview__/runtime-install":
            try:
                return self.send_json(job_status(query.get("id", [""])[0]))
            except ValueError as error:
                return self.send_json({"error": str(error)}, 404)
        if url.path != "/__preview__/executables":
            return self.send_json({"error": "Unknown preview endpoint."}, 404)
        command = query.get("command", [""])[0]
        cwd = query.get("cwd", [""])[0]
        if not command or len(command) > 4096 or "\0" in command or len(cwd) > 4096:
            return self.send_json({"error": "Invalid executable name."}, 400)
        try:
            return self.send_json(discover_executables(command, cwd))
        except OSError:
            return self.send_json({"error": "PATH discovery failed."}, 500)

    def do_POST(self):
        path = urllib.parse.urlsplit(self.path).path
        if path not in {"/__preview__/runtime-install", "/__preview__/application-help"} or not self.is_local_request():
            return self.send_json({"error": "Local preview requests only."}, 403)
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= (128 * 1024 if path.endswith("application-help") else 8192):
                raise ValueError("요청 크기를 확인하세요.")
            data = json.loads(self.rfile.read(length))
            if not isinstance(data, dict):
                raise ValueError("잘못된 요청입니다.")
            if path == "/__preview__/application-help":
                return self.send_json(application_help(data))
            if data.get("action") == "cancel":
                return self.send_json(cancel_install(data.get("id", "")))
            family, release_id, destination = data.get("family"), data.get("releaseId"), data.get("destination")
            if not all(isinstance(value, str) and value and len(value) <= 4096 and "\0" not in value for value in (family, release_id, destination)):
                raise ValueError("버전과 설치 경로를 확인하세요.")
            return self.send_json(start_install(family, release_id, destination), 202)
        except (ValueError, TypeError) as error:
            return self.send_json({"error": str(error)}, 400)
        except Exception as error:
            return self.send_json({"error": str(error)}, 500)

    def send_json(self, value, status=200):
        body = json.dumps(value, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=4317)
    arguments = parser.parse_args()
    handler = functools.partial(PreviewHandler, directory=str(ROOT))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", arguments.port), handler)
    print(f"Local HTML preview and PATH discovery: http://127.0.0.1:{arguments.port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
