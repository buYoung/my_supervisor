"""Bounded help discovery for known runtimes, cached by executable identity."""

import datetime
import hashlib
import json
import os
import re
import tempfile
import threading
from pathlib import Path
from help_parser import PARSER_REVISION, parse_help
from help_process import help_environment, run_help

HELP_ARGUMENTS = {
    "node": [("--help",)], "python": [("-I", "--help")],
    "java": [("-help",), ("-X",)], "go": [("help",)],
    "ruby": [("--help",)], "bun": [("--help",)], "deno": [("--help",)],
}
CACHE_ROOT = Path(tempfile.gettempdir()) / "my-supervisor-preview" / "runtime-help"
CACHE = {}
LOCK = threading.Lock()


def help_options(text, command):
    return parse_help(text, command)["options"]


def positional_help(family, text, usage):
    if family == "node" and re.search(r"\[\s*script\.js\s*\]", usage):
        return {"kind": "script", "label": "실행 스크립트", "description": "Node.js가 실행할 JavaScript 진입 파일입니다.",
                "usage": usage, "inlineOptions": ["-e", "--eval", "-p", "--print", "--run"]}
    if family == "python" and re.search(r"\bfile\b", usage):
        return {"kind": "script", "label": "실행 스크립트", "description": "Python이 실행할 스크립트 파일입니다.",
                "usage": usage, "inlineOptions": ["-c", "-m"]}
    if family == "java" and re.search(r"mainclass|class", usage, re.I):
        return {"kind": "class", "label": "메인 클래스", "description": "Java가 실행할 메인 클래스입니다.",
                "usage": usage, "inlineOptions": ["-jar", "-m", "--module"]}
    if family == "go" and "<command>" in usage:
        return {"kind": "command", "label": "하위 명령", "description": "Go가 수행할 하위 명령입니다.",
                "usage": usage, "inlineOptions": []}
    return None


def read_help_output(path, arguments):
    output, status = run_help([path, *arguments], tempfile.gettempdir(), help_environment(), 2)
    return output if status == "finished" else ""


def executable_help(entry):
    family = entry["family"]
    if family not in HELP_ARGUMENTS or family == "java" and entry["resolvedPath"] == "/usr/bin/java":
        return {"status": "unsupported", "options": {}, "positional": None}
    try:
        stat = os.stat(entry["resolvedPath"])
        identity = [5, PARSER_REVISION, family, entry["resolvedPath"], stat.st_mtime_ns, stat.st_size, entry.get("version")]
        key = hashlib.sha256(json.dumps(identity).encode()).hexdigest()
    except OSError:
        return {"status": "unavailable", "options": {}, "positional": None}
    with LOCK:
        if key in CACHE:
            return {**CACHE[key], "cacheHit": True}
        cache_file = CACHE_ROOT / (key + ".json")
        try:
            if cache_file.stat().st_size < 512 * 1024:
                cached = json.loads(cache_file.read_text())
                if cached.get("key") == key:
                    CACHE[key] = cached
                    return {**cached, "cacheHit": True}
        except (OSError, ValueError):
            pass
        outputs = []
        options = {}
        for arguments in HELP_ARGUMENTS[family]:
            try:
                output = read_help_output(entry["path"], arguments)
            except OSError:
                output = ""
            outputs.append(output)
            options.update(help_options(output, " ".join(arguments)))
        text = "\n".join(outputs)
        usage_match = re.search(r"(?im)^usage:\s*(.+)$", text)
        usage = usage_match.group(0).strip() if usage_match else ""
        result = {"key": key, "status": "ready" if options or usage else "unavailable", "family": family,
                  "label": entry["label"], "version": entry.get("version"), "path": entry["path"],
                  "options": options, "positional": positional_help(family, text, usage), "usage": usage,
                  "cachedAt": datetime.datetime.now(datetime.timezone.utc).isoformat()}
        if len(CACHE) >= 64:
            CACHE.clear()
        if result["status"] == "ready":
            CACHE[key] = result
            try:
                CACHE_ROOT.mkdir(parents=True, exist_ok=True)
                temporary = cache_file.with_suffix(".tmp")
                temporary.write_text(json.dumps(result, ensure_ascii=False))
                temporary.replace(cache_file)
            except OSError:
                pass
        return {**result, "cacheHit": False}
