# fmt: off

import os
import pty
import select
import time
import re


def test_interactive_ctrl_c_twice_prints_ctrl_d_hint(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"

    pid, master_fd = pty.fork()
    if pid == 0:
        os.execvpe(shell, [shell, "-interactive", "--init", "/dev/null"], env)

    buf = bytearray()
    recent = bytearray()
    read_pos = 0

    def strip_ansi(s: str) -> str:
        s = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", s)
        s = re.sub(r"\x1b\][^\x07]*\x07", "", s)
        return s

    def read_until(needle: str, timeout_s: float = 10.0) -> str:
        nonlocal read_pos, buf, recent
        start = read_pos
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
            r, _, _ = select.select([master_fd], [], [], 0.1)
            if not r:
                continue
            chunk = os.read(master_fd, 4096)
            if not chunk:
                continue
            buf.extend(chunk)
            recent.extend(chunk)
            if len(recent) > 256:
                del recent[:-256]
            if b"\x1b[6n" in recent:
                os.write(master_fd, b"\x1b[1;1R")
                recent = recent.replace(b"\x1b[6n", b"")
            if needle_b in buf[start:]:
                read_pos = len(buf)
                return buf[start:].decode("utf-8", errors="ignore")

    def write_bytes(b: bytes):
        os.write(master_fd, b)

    try:
        read_until("D ")
        write_bytes(b"\x03")
        read_until("D ")
        write_bytes(b"\x03")
        out = strip_ansi(read_until("D "))
        assert "Interrupted, use Ctrl+D to exit" in out

        write_bytes(b".quit\r")
        deadline = time.time() + 10.0
        while time.time() < deadline:
            exited_pid, _ = os.waitpid(pid, os.WNOHANG)
            if exited_pid != 0:
                break
            time.sleep(0.05)
    finally:
        try:
            os.close(master_fd)
        except OSError:
            pass


# fmt: on

