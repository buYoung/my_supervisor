"""Local script/CLI help discovery. Never run the configured application arguments."""

import datetime
import hashlib
import json
import os
import re
import shutil
import tempfile
import threading
import time
from pathlib import Path

from help_parser import PARSER_REVISION, parse_help
from help_process import help_environment, run_help
from runtime_help import executable_help

CACHE_ROOT = Path(tempfile.gettempdir()) / "my-supervisor-preview" / "application-help"
CACHE = {}
LOCK = threading.Lock()
RUNTIMES = {"node", "python", "ruby", "perl", "php", "lua", "luajit", "bash", "zsh", "sh", "fish", "bun", "deno", "java", "dotnet", "go", "tsx", "ts-node"}
VALUE_OPTIONS = {"-r", "--require", "--import", "--loader", "--experimental-loader", "--conditions", "--env-file", "--env-file-if-exists", "--inspect-port", "--inspect", "--inspect-brk", "--title", "--icu-data-dir", "--openssl-config", "--diagnostic-dir", "--max-old-space-size", "-X", "-W", "-I", "-F", "-d", "-c", "-cp", "-classpath", "--class-path", "-p", "--module-path", "--add-modules", "--add-exports", "--add-opens", "--project", "-P", "--config"}
NO_VALUE_OPTIONS = {"--inspect", "--inspect-brk", "--inspect-wait", "--watch", "--watch-preserve-output"}
INLINE_OPTIONS = {"node": {"-e", "--eval", "-p", "--print", "--run"}, "python": {"-c"}, "ruby": {"-e"}, "perl": {"-e", "-E"}, "php": {"-r"}, "bash": {"-c"}, "zsh": {"-c"}, "sh": {"-c"}, "fish": {"-c"}}
REASONS = {"missing": "작업 디렉터리 또는 실행 파일을 찾을 수 없습니다.", "unsupported": "조회할 스크립트가 없거나 도움말 자동 조회를 지원하지 않는 실행 형식입니다.", "unparsed": "옵션 설명이 있는 도움말을 찾지 못했습니다.", "timeout": "도움말 응답 시간이 초과되었습니다.", "too-large": "도움말 출력 크기가 제한을 초과했습니다.", "unavailable": "도움말을 읽을 수 없습니다."}


def unavailable(status, **metadata):
    return {"status": status, "options": {}, "commands": {}, "reason": REASONS.get(status, REASONS["unavailable"]), **metadata}


def runtime_family(path):
    name = Path(path).name
    if re.fullmatch(r"python(?:\d+(?:\.\d+)*)?", name):
        return "python"
    if name == "nodejs":
        return "node"
    return name


def existing_file(value, cwd):
    path = Path(value).expanduser()
    if not path.is_absolute():
        path = cwd / path
    return path.resolve() if path.is_file() else None


def resolve_target(argv, cwd, environment):
    executable = existing_file(argv[0], cwd) if "/" in argv[0] else shutil.which(argv[0], path=environment.get("PATH"))
    if not executable:
        return unavailable("missing")
    executable = Path(executable).resolve()
    family = runtime_family(argv[0])
    if family not in RUNTIMES or family == "go" and (len(argv) < 2 or argv[1] != "run"):
        return {"status": "resolved", "prefix": [str(executable)], "label": Path(argv[0]).name,
                "kind": "command", "family": family, "argumentWordIndex": 1, "files": [executable], "arguments": argv[1:]}
    if family == "java" and str(executable) == "/usr/bin/java":
        return unavailable("unsupported")
    runtime_options = executable_help({"family": family, "path": str(executable), "resolvedPath": str(executable), "label": family}).get("options", {})
    prefix, index = [str(executable)], 1
    while index < len(argv):
        token = argv[index]
        name = token.split("=", 1)[0]
        inline = INLINE_OPTIONS.get(family, set())
        has_inline_code = any(len(option) == 2 and token.startswith(option) for option in inline)
        has_shell_code = family in {"bash", "zsh", "sh", "fish"} and bool(re.fullmatch(r"-[A-Za-z]*c[A-Za-z]*", token))
        if has_inline_code or has_shell_code or name in {"--help", "-h", "--version", "-version"} | inline or family == "node" and name in {"--test", "--check", "-c"}:
            return unavailable("unsupported")
        if family == "python" and token == "-m":
            if index + 1 >= len(argv) or not re.fullmatch(r"[\w.]+", argv[index + 1]):
                return unavailable("unsupported")
            return {"status": "resolved", "prefix": prefix + argv[index:index + 2], "label": argv[index + 1],
                    "kind": "script", "family": family, "entryWordIndex": index + 1, "argumentWordIndex": index + 2,
                    "files": [executable], "arguments": argv[index + 2:]}
        if family == "java" and token in {"-jar", "-m", "--module"} or family == "php" and token == "-f":
            prefix.append(token)
            index += 1
            break
        if family in {"bun", "deno", "go"} and token == "run":
            prefix.append(token)
            index += 1
            continue
        if token == "--":
            prefix.append(token)
            index += 1
            break
        if not token.startswith("-") or token == "-":
            break
        # An inspector or watcher is not needed when asking a script for help.
        if family == "node" and name in NO_VALUE_OPTIONS:
            index += 1
            continue
        prefix.append(token)
        documented = runtime_options.get(name)
        takes_value = documented["takesValue"] if documented else name in VALUE_OPTIONS
        if "=" not in token and takes_value:
            if index + 1 >= len(argv):
                return unavailable("unsupported")
            prefix.append(argv[index + 1])
            index += 1
        index += 1
    if index >= len(argv):
        return unavailable("unsupported")
    entry = existing_file(argv[index], cwd)
    if not entry:
        # Java class/module names are resolved by the selected class/module path.
        if family != "java" or "-jar" in prefix:
            return unavailable("missing")
        if not re.fullmatch(r"[\w.$/]+", argv[index]):
            return unavailable("unsupported")
    if family == "go" and ("run" not in prefix or not str(entry).endswith(".go")):
        return unavailable("unsupported")
    return {"status": "resolved", "prefix": prefix + [str(entry) if entry else argv[index]],
            "label": Path(argv[index]).name, "kind": "script", "family": family,
            "entryWordIndex": index, "argumentWordIndex": index + 1,
            "files": [executable] + ([entry] if entry else []), "arguments": argv[index + 1:]}


def fingerprints(target, cwd):
    paths = set(target["files"])
    for directory in {cwd, *(path.parent for path in target["files"])}:
        for parent in [directory, *list(directory.parents)[:2]]:
            for name in ("package.json", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "pyproject.toml", "requirements.txt", "go.mod", "go.sum", "Gemfile.lock"):
                path = parent / name
                if path.is_file():
                    paths.add(path)
    result = []
    for path in sorted(paths):
        stat = path.stat()
        result.append([str(path), stat.st_size, stat.st_mtime_ns])
    return result


def cached_help(target, cwd, environment, context, force):
    identity = [PARSER_REVISION, target["prefix"], context, str(cwd), fingerprints(target, cwd),
                hashlib.sha256(json.dumps(environment, sort_keys=True).encode()).hexdigest()]
    key = hashlib.sha256(json.dumps(identity, sort_keys=True).encode()).hexdigest()
    cache_file = CACHE_ROOT / (key + ".json")
    with LOCK:
        cached = CACHE.get(key)
        if not cached:
            try:
                if cache_file.stat().st_size < 768 * 1024:
                    cached = json.loads(cache_file.read_text())
            except (OSError, ValueError):
                pass
        if not force and cached and cached.get("expiresAt", 0) > time.time():
            return {**cached, "cacheHit": True}
        help_args = [*context, "-h"] if target["family"] == "git" else ["help", *context] if target["kind"] == "command" and target["family"] == "go" else [*context, "--help"]
        output, status = run_help([*target["prefix"], *help_args], cwd, environment)
        parsed = parse_help(output, " ".join(help_args))
        status = "ready" if status == "finished" and parsed["isHelp"] else "unparsed" if status == "finished" else status
        result = {**parsed, "key": key, "status": status, "kind": target["kind"], "family": target["family"],
                  "label": " ".join([target["label"], *context]), "context": context,
                  "entryWordIndex": target.get("entryWordIndex"), "argumentWordIndex": target["argumentWordIndex"],
                  "path": target["prefix"][-1], "sourceCommand": " ".join([target["label"], *help_args]),
                  "cachedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  "expiresAt": time.time() + (86400 if status == "ready" else 120),
                  "reason": "" if status == "ready" else REASONS.get(status, REASONS["unavailable"])}
        if status != "ready":
            result["options"], result["commands"] = {}, {}
        if len(CACHE) >= 128:
            CACHE.clear()
        CACHE[key] = result
        try:
            CACHE_ROOT.mkdir(parents=True, exist_ok=True)
            temporary = cache_file.with_suffix(".tmp")
            temporary.write_text(json.dumps(result, ensure_ascii=False))
            temporary.replace(cache_file)
        except OSError:
            pass
        return {**result, "cacheHit": False}


def next_subcommand(arguments, options):
    index = 0
    while index < len(arguments):
        token = arguments[index]
        if token == "--":
            return None, []
        if not token.startswith("-"):
            return token, arguments[index + 1:]
        details = options.get(token.split("=", 1)[0])
        if not details:
            return None, []
        index += 2 if details["takesValue"] and "=" not in token else 1
    return None, []


def application_help(data):
    argv, overrides = data.get("argv"), data.get("environment", {})
    if not isinstance(argv, list) or not 0 < len(argv) <= 512 or not all(isinstance(value, str) and len(value) <= 8192 and "\0" not in value for value in argv):
        raise ValueError("실행 명령을 확인하세요.")
    if not isinstance(overrides, dict) or len(overrides) > 512 or not all(isinstance(key, str) and re.fullmatch(r"[A-Za-z_]\w*", key) and isinstance(value, str) and len(value) <= 32768 and "\0" not in value for key, value in overrides.items()):
        raise ValueError("환경변수를 확인하세요.")
    raw_cwd = data.get("cwd", "")
    if not isinstance(raw_cwd, str) or len(raw_cwd) > 4096 or "\0" in raw_cwd:
        raise ValueError("작업 디렉터리를 확인하세요.")
    cwd = Path(raw_cwd or os.getcwd()).expanduser().resolve()
    if not cwd.is_dir():
        return unavailable("missing")
    environment = help_environment(overrides)
    target = resolve_target(argv, cwd, environment)
    if target["status"] != "resolved":
        return target
    result = cached_help(target, cwd, environment, [], data.get("refresh") is True)
    remaining, context = target["arguments"], []
    for _ in range(4):
        candidate, rest = next_subcommand(remaining, result["options"])
        if not candidate or candidate not in result["commands"]:
            break
        context = [*context, candidate]
        child = cached_help(target, cwd, environment, context, data.get("refresh") is True)
        if child["status"] != "ready":
            break
        child["options"] = {**result["options"], **child["options"]}
        result, remaining = child, rest
    return result
