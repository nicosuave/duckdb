# fmt: off

import os
import pty
import select
import time
import re


PROMPT_RE = r"memory(?:\.[^\r\n ]+)? D ?"


def _strip_ansi(s: str) -> str:
    s = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", s)
    s = re.sub(r"\x1b\][^\x07]*\x07", "", s)
    return s


def _read_until(fd: int, pid: int, needle: str, timeout_s: float = 10.0) -> str:
    buf = bytearray()
    recent = bytearray()
    deadline = time.time() + timeout_s
    needle_b = needle.encode("utf-8")

    while True:
        if time.time() >= deadline:
            raise AssertionError(
                f"timeout waiting for {needle!r}, output so far:\n{buf.decode('utf-8', errors='ignore')}"
            )
        exited_pid, status = os.waitpid(pid, os.WNOHANG)
        if exited_pid != 0:
            raise AssertionError(
                f"process exited while waiting for {needle!r} (status={status}), output so far:\n{buf.decode('utf-8', errors='ignore')}"
            )

        r, _, _ = select.select([fd], [], [], 0.1)
        if not r:
            continue
        chunk = os.read(fd, 4096)
        if not chunk:
            continue
        buf.extend(chunk)
        recent.extend(chunk)
        if len(recent) > 256:
            del recent[:-256]
        if b"\x1b[6n" in recent:
            os.write(fd, b"\x1b[1;1R")
            recent = recent.replace(b"\x1b[6n", b"")
        if needle_b in buf:
            return buf.decode("utf-8", errors="ignore")


def _read_until_re(fd: int, pid: int, pattern: str, timeout_s: float = 10.0) -> str:
    buf = bytearray()
    recent = bytearray()
    deadline = time.time() + timeout_s
    regex = re.compile(pattern)

    while True:
        if time.time() >= deadline:
            raise AssertionError(
                f"timeout waiting for {pattern!r}, output so far:\n{buf.decode('utf-8', errors='ignore')}"
            )
        exited_pid, status = os.waitpid(pid, os.WNOHANG)
        if exited_pid != 0:
            raise AssertionError(
                f"process exited while waiting for {pattern!r} (status={status}), output so far:\n{buf.decode('utf-8', errors='ignore')}"
            )

        r, _, _ = select.select([fd], [], [], 0.1)
        if not r:
            continue
        chunk = os.read(fd, 4096)
        if not chunk:
            continue
        buf.extend(chunk)
        recent.extend(chunk)
        if len(recent) > 256:
            del recent[:-256]
        if b"\x1b[6n" in recent:
            os.write(fd, b"\x1b[1;1R")
            recent = recent.replace(b"\x1b[6n", b"")

        text = _strip_ansi(buf.decode("utf-8", errors="ignore"))
        if regex.search(text):
            return text


def test_interactive_ctrl_w_deletes_previous_word(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"

    pid, master_fd = pty.fork()
    if pid == 0:
        os.execvpe(shell, [shell, "-interactive", "--init", "/dev/null"], env)

    try:
        _read_until(master_fd, pid, "D ")
        os.write(master_fd, b".print hello world")
        os.write(master_fd, b"\x17")  # Ctrl-W (delete previous word)
        os.write(master_fd, b"duckdb\r")
        pattern = rf"(?:\r?\n)hello duckdb(?:\r?\n){PROMPT_RE}"
        out = _read_until_re(master_fd, pid, pattern)
        assert re.search(pattern, out)
        os.write(master_fd, b".quit\r")
    finally:
        try:
            os.close(master_fd)
        except OSError:
            pass


# fmt: on
