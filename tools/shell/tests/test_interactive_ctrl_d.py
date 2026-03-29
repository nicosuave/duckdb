# fmt: off

import os
import pty
import select
import time


def test_interactive_ctrl_d_exits(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "dumb"

    master_fd, slave_fd = pty.openpty()
    try:
        pid = os.fork()
        if pid == 0:
            os.execvpe(shell, [shell, "-interactive", "--init", "/dev/null"], env)
    finally:
        os.close(slave_fd)

    buf = bytearray()
    deadline = time.time() + 10.0
    wrote = False
    try:
        while True:
            exited_pid, status = os.waitpid(pid, os.WNOHANG)
            if exited_pid != 0:
                # exited
                break
            if time.time() >= deadline:
                raise AssertionError(f"timeout, output so far:\n{buf.decode('utf-8', errors='ignore')}")

            r, _, _ = select.select([master_fd], [], [], 0.1)
            if r:
                chunk = os.read(master_fd, 4096)
                if chunk:
                    buf.extend(chunk)

            if not wrote and b"D " in buf:
                # Ctrl-D on an empty prompt should exit.
                os.write(master_fd, b"\x04")
                wrote = True

        text = buf.decode("utf-8", errors="ignore")
        assert "Interrupted, use Ctrl+D to exit" not in text
    finally:
        try:
            os.close(master_fd)
        except OSError:
            pass


# fmt: on

