# fmt: off

import pytest
import subprocess
import sys
from typing import List
from conftest import ShellTest
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


def _spawn_interactive(shell, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"

    pid, master_fd = pty.fork()
    if pid == 0:
        os.execvpe(shell, [shell, "-interactive", "--init", "/dev/null"], env)
    return pid, master_fd


def test_prompt_unterminated_bracket(shell):
    test = (
        ShellTest(shell)
        .statement('.prompt {')
    )
    result = test.run()
    result.check_stderr("unterminated bracket")

def test_prompt_unterminated_escape(shell):
    test = (
        ShellTest(shell)
        .statement('.prompt \\')
    )
    result = test.run()
    result.check_stderr("unterminated")

def test_prompt_invalid_option(shell):
    test = (
        ShellTest(shell)
        .statement('.prompt "{yyy}"')
    )
    result = test.run()
    result.check_stderr("yyy")

def test_prompt_missing_query(shell):
    test = (
        ShellTest(shell)
        .statement('.prompt "{sql}"')
    )
    result = test.run()
    result.check_stderr("sql requires a parameter")

def test_prompt_invalid_color(shell):
    test = (
        ShellTest(shell)
        .statement('.prompt "{color:xxx}"')
    )
    result = test.run()
    result.check_stderr("xxx")


def test_prompt_default_database_and_schema(shell, tmp_path):
    pid, master_fd = _spawn_interactive(shell, tmp_path)

    try:
        out = _read_until_re(master_fd, pid, PROMPT_RE)
        assert re.search(PROMPT_RE, out)
        os.write(master_fd, b"create schema beta; use beta;\r")
        out = _read_until_re(master_fd, pid, r"memory\.beta D ?")
        assert "memory.beta D" in out
        os.write(master_fd, b".quit\r")
    finally:
        try:
            os.close(master_fd)
        except OSError:
            pass


def test_prompt_sql_setting_and_max_length(shell, tmp_path):
    pid, master_fd = _spawn_interactive(shell, tmp_path)

    try:
        _read_until_re(master_fd, pid, PROMPT_RE)
        os.write(master_fd, b'.prompt "{sql:select 42}> "\r')
        out = _read_until_re(master_fd, pid, r"(?:^|\r?\n)42> ")
        assert re.search(r"(?:^|\r?\n)42> ", out)
        os.write(master_fd, b'.prompt "{sql:select NULL}> "\r')
        out = _read_until_re(master_fd, pid, r"(?:^|\r?\n)#NULL#> ")
        assert re.search(r"(?:^|\r?\n)#NULL#> ", out)
        os.write(master_fd, b'.prompt "{max_length:5}abcdef "\r')
        out = _read_until_re(master_fd, pid, r"(?:^|\r?\n)abcde\.\.\. D ")
        assert re.search(r"(?:^|\r?\n)abcde\.\.\. D ", out)
        os.write(master_fd, b".quit\r")
    finally:
        try:
            os.close(master_fd)
        except OSError:
            pass


def test_progress_bar_component_validation(shell):
    valid = (
        ShellTest(shell)
        .statement('.progress_bar --add "{setting:progress_bar_percentage} {align:right}{setting:eta}"')
        .run()
    )
    assert valid.status_code == 0, valid.stderr

    invalid = (
        ShellTest(shell)
        .statement('.progress_bar --add "{bogus:x}"')
        .run()
    )
    assert invalid.status_code == 1
    invalid.check_stderr("Unknown bracket type bogus")

# fmt: on
