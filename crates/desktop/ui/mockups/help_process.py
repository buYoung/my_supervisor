"""Run a help command with a deadline and a bounded combined output stream."""

import os
import selectors
import signal
import subprocess
import time

MAX_HELP_BYTES = 256 * 1024


def help_environment(overrides=None):
    environment = dict(os.environ)
    environment.update(overrides or {})
    for name in ("NODE_OPTIONS", "PYTHONSTARTUP", "RUBYOPT", "JAVA_TOOL_OPTIONS", "JDK_JAVA_OPTIONS",
                 "_JAVA_OPTIONS", "BUN_OPTIONS", "LD_PRELOAD", "DYLD_INSERT_LIBRARIES"):
        environment.pop(name, None)
    environment.update(NO_COLOR="1", FORCE_COLOR="0", TERM="dumb", COLUMNS="160", LC_ALL="C", LANG="C",
                       GOTOOLCHAIN="local", GOPROXY="off", GOSUMDB="off", PAGER="cat", GIT_PAGER="cat")
    return environment


def run_help(argv, cwd, environment, timeout_seconds=3):
    output, status = bytearray(), "finished"
    try:
        process = subprocess.Popen(argv, cwd=cwd, env=environment, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    except OSError:
        return "", "unavailable"
    deadline = time.monotonic() + timeout_seconds
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    status = "timeout"
                    break
                for key, _ in selector.select(min(remaining, 0.2)):
                    chunk = os.read(key.fileobj.fileno(), 8192)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    output.extend(chunk)
                    if len(output) > MAX_HELP_BYTES:
                        status = "too-large"
                        break
                if status != "finished":
                    break
            if status == "finished":
                try:
                    process.wait(timeout=max(0.01, deadline - time.monotonic()))
                except subprocess.TimeoutExpired:
                    status = "timeout"
    finally:
        # Also reap children which kept the pipe open or detached from their parent.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
        process.stdout.close()
    return output[:MAX_HELP_BYTES].decode("utf-8", "replace"), status
