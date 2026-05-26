# fmt: off

import os
import pty
import select
import subprocess
import time
import re


def _pty_run(shell, args, env, send_lines, send_after=None, timeout_s=10.0):
    master_fd, slave_fd = pty.openpty()
    try:
        proc = subprocess.Popen(
            [shell, *args],
            stdin=slave_fd,
            stdout=slave_fd,
            stderr=slave_fd,
            env=env,
            close_fds=True,
        )
    finally:
        os.close(slave_fd)

    buf = bytearray()
    recent = bytearray()
    deadline = time.time() + timeout_s
    sent_idx = 0
    if send_after is None:
        triggers = ["D "] * len(send_lines)
    elif isinstance(send_after, str):
        triggers = [send_after] * len(send_lines)
    else:
        triggers = list(send_after)
        assert len(triggers) == len(send_lines)
    trigger_bytes = [t.encode("utf-8") for t in triggers]
    search_start = 0

    while True:
        if proc.poll() is not None:
            break

        now = time.time()
        if now >= deadline:
            proc.kill()
            raise AssertionError(f"pty timeout after {timeout_s}s, output so far:\n{buf.decode('utf-8', errors='ignore')}")

        r, _, _ = select.select([master_fd], [], [], 0.1)
        if not r:
            continue

        try:
            chunk = os.read(master_fd, 4096)
        except OSError:
            break
        if not chunk:
            break
        buf.extend(chunk)
        recent.extend(chunk)
        if len(recent) > 256:
            del recent[:-256]
        # Reply to terminal cursor position queries used by linenoise terminal sizing.
        if b"\x1b[6n" in recent:
            os.write(master_fd, b"\x1b[1;1R")
            recent = recent.replace(b"\x1b[6n", b"")
        if sent_idx < len(send_lines):
            if trigger_bytes[sent_idx] in buf[search_start:]:
                os.write(master_fd, (send_lines[sent_idx] + "\r").encode("utf-8"))
                sent_idx += 1
                search_start = len(buf)

    try:
        os.close(master_fd)
    except OSError:
        pass

    return buf.decode("utf-8", errors="ignore")


def test_interactive_startup_banner_transient_db(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "dumb"

    out = _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=env,
        send_lines=[".quit"],
        send_after="D ",
        timeout_s=10.0,
    )

    assert 'Enter ".help" for usage hints.' in out
    assert "Connected to a transient in-memory database." in out
    assert 'Use ".open FILENAME" to reopen on a persistent database.' in out


def test_interactive_multiline_statement_prompts(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "dumb"

    out = _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=env,
        send_lines=[
            ".mode csv",
            "select 1",
            ";",
            ".quit",
        ],
        send_after=["D ", "D ", "· ", "D "],
        timeout_s=10.0,
    )

    assert "· " in out
    assert "\n1" in out or "1\r\n" in out or "1\n" in out


def test_interactive_startup_text_none_suppresses_help(shell, tmp_path):
    rc_path = tmp_path / ".duckdbrc"
    rc_path.write_text(".startup_text none\n", encoding="utf-8")

    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "dumb"

    out = _pty_run(
        shell=shell,
        args=["-interactive"],
        env=env,
        send_lines=[".quit"],
        send_after="D ",
        timeout_s=10.0,
    )

    assert 'Enter ".help" for usage hints.' not in out


def test_interactive_startup_text_version_suppresses_help(shell, tmp_path):
    rc_path = tmp_path / ".duckdbrc"
    rc_path.write_text(".startup_text version\n", encoding="utf-8")

    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "dumb"

    out = _pty_run(
        shell=shell,
        args=["-interactive"],
        env=env,
        send_lines=[".quit"],
        send_after="D ",
        timeout_s=10.0,
    )

    assert "DuckDB " in out
    assert 'Enter ".help" for usage hints.' not in out


def test_interactive_duckdbrc_loading_resources_message(shell, tmp_path):
    rc_path = tmp_path / ".duckdbrc"
    rc_path.write_text(".mode csv\n", encoding="utf-8")

    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "dumb"

    out = _pty_run(
        shell=shell,
        args=["-interactive"],
        env=env,
        send_lines=[".quit"],
        send_after="D ",
        timeout_s=10.0,
    )

    expected = f"-- Loading resources from {rc_path}"
    assert expected in out
    assert out.count("-- Loading resources from") == 1


def test_interactive_cmd_runs_before_reading_stdin(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "dumb"

    out = _pty_run(
        shell=shell,
        args=[
            "-interactive",
            "--init",
            "/dev/null",
            "-cmd",
            ".mode csv",
            "-cmd",
            "select 1 as cmd_option_works;",
        ],
        env=env,
        send_lines=[".quit"],
        send_after="D ",
        timeout_s=10.0,
    )

    assert "cmd_option_works" in out
    assert "┌" not in out


def test_interactive_edit_opens_editor_and_executes(shell, tmp_path):
    editor = tmp_path / "duckdb_editor.sh"
    editor.write_text(
        "#!/usr/bin/env sh\n"
        "set -eu\n"
        "cat >\"$1\" <<'SQL'\n"
        "select 42 as edited;\n"
        "SQL\n",
        encoding="utf-8",
    )
    os.chmod(editor, 0o755)

    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"
    env["DUCKDB_EDITOR"] = str(editor)

    out = _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=env,
        send_lines=[".edit", ".quit"],
        send_after=["D ", "D "],
        timeout_s=20.0,
    )

    assert "edited" in out


def test_interactive_edit_escape_alias_opens_editor_and_executes(shell, tmp_path):
    editor = tmp_path / "duckdb_editor_alias.sh"
    editor.write_text(
        "#!/usr/bin/env sh\n"
        "set -eu\n"
        "cat >\"$1\" <<'SQL'\n"
        "select 84 as edited_alias;\n"
        "SQL\n",
        encoding="utf-8",
    )
    os.chmod(editor, 0o755)

    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"
    env["DUCKDB_EDITOR"] = str(editor)

    out = _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=env,
        send_lines=["\\e", ".quit"],
        send_after=["D ", "D "],
        timeout_s=20.0,
    )

    assert "edited_alias" in out


def test_interactive_sql_highlighting_emits_ansi(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"

    out = _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=env,
        send_lines=["select 1 as highlighted;", ".quit"],
        send_after=["D ", "D "],
        timeout_s=10.0,
    )

    assert "\x1b[" in out
    stripped = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", out)
    assert "select 1 as highlighted;" in stripped


def test_interactive_highlight_colors_update_sql_editor(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"

    out = _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=env,
        send_lines=[".highlight_colors keyword red", "select 123 as v;", ".quit"],
        send_after=["D ", "D ", "D "],
        timeout_s=10.0,
    )

    assert "\x1b[31mselect " in out
    assert "\x1b[31mas " in out
    stripped = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", out)
    assert "select 123 as v;" in stripped


def test_interactive_highlight_off_disables_sql_editor_highlighting(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"

    out = _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=env,
        send_lines=[".highlight off", "select 123 as v;", ".quit"],
        send_after=["D ", "D ", "D "],
        timeout_s=10.0,
    )

    stripped = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", out)
    assert "D select 123 as v;" in stripped
    assert not re.search(r"(?:\x1b\[[0-9;]*m)+select ", out)
    assert not re.search(r"(?:\x1b\[[0-9;]*m)+as ", out)


def test_interactive_reverse_search(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"

    pid, master_fd = pty.fork()
    if pid == 0:
        os.execvpe(shell, [shell, "-interactive", "--init", "/dev/null"], env)

    def read_until(needle: str, timeout_s: float = 10.0) -> str:
        buf = bytearray()
        recent = bytearray()
        deadline = time.time() + timeout_s
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
            text = buf.decode("utf-8", errors="ignore")
            if needle in text:
                return text

    def write_bytes(b: bytes):
        os.write(master_fd, b)

    def strip_ansi(s: str) -> str:
        # CSI ... (ESC [ ...), plus OSC ... BEL (ESC ] ... BEL).
        s = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", s)
        s = re.sub(r"\x1b\][^\x07]*\x07", "", s)
        return s

    try:
        read_until("D ")
        write_bytes(b".mode csv\r")
        read_until("D ")
        write_bytes(b"select 123 as v;\r")
        read_until("D ")

        # Ctrl-R initiates search; linenoise renders a "search ... > " prompt and "(type to search)" placeholder.
        write_bytes(b"\x12")
        read_until("(type to search)")

        write_bytes(b"123")
        rendered = read_until("1/1> ")
        assert "select 123 as v;" in strip_ansi(rendered)

        # Ctrl-G cancels search.
        write_bytes(b"\x07")
        read_until("D ")

        write_bytes(b"123\x12")
        seeded = read_until("1/1> ")
        assert "select 123 as v;" in strip_ansi(seeded)

        write_bytes(b"\x07")
        read_until("D ")

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
