# fmt: off

import os
import pytest
from conftest import ShellTest
from test_interactive_startup import _pty_run

def test_pager_status_default(shell):
    """Test that pager status shows 'automatic' by default"""
    test = (
        ShellTest(shell)
            .statement('.pager')
    )
    result = test.run()
    result.check_stdout('Pager mode: automatic')
    result.check_stdout('Trigger pager when rows exceed 50 or result set is wider than terminalPager command:')


def test_pager_help(shell):
    """Test that .help shows pager documentation"""
    test = (
        ShellTest(shell)
            .statement('.help pager')
    )
    result = test.run()
    result.check_stdout('DUCKDB_PAGER')


def test_pager_off_explicit(shell):
    """Test setting pager explicitly to off"""
    test = (
        ShellTest(shell)
            .statement('.pager off')
            .statement('.pager')
    )
    result = test.run()
    result.check_stdout('Pager mode: off')
    result.check_stdout('Pager command:')


def test_pager_column_threshold_rejected(shell):
    test = (
        ShellTest(shell)
            .statement('.pager set_column_threshold 1')
    )
    result = test.run()
    assert result.status_code == 1
    result.check_stderr("Invalid usage of command '.pager'")

def test_pager_on_with_pager_env(shell):
    """Test that pager uses PAGER environment variable"""
    test = (
        ShellTest(shell)
            .statement('.pager on')
            .statement('.pager')
    )
    test.environment['PAGER'] = 'less'
    result = test.run()
    result.check_stdout('less')


def test_pager_on_with_duckdb_pager_env(shell):
    """Test that DUCKDB_PAGER takes precedence over PAGER"""
    test = (
        ShellTest(shell)
            .statement('.pager on')
            .statement('.pager')
    )
    test.environment['PAGER'] = 'less'
    test.environment['DUCKDB_PAGER'] = 'less -SR'
    result = test.run()
    result.check_stdout('less -SR')


def test_pager_duckdb_pager_priority(shell):
    """Test DUCKDB_PAGER shows in status even when pager is off"""
    test = (
        ShellTest(shell)
            .statement('.pager on')
            .statement('.pager')
    )
    test.environment['DUCKDB_PAGER'] = 'less -R'
    result = test.run()
    result.check_stdout('less -R')


def test_pager_custom_command(shell):
    """Test setting a custom pager command"""
    test = (
        ShellTest(shell)
            .statement(".pager 'cat'")
            .statement('.pager')
    )
    result = test.run()
    result.check_stdout('cat')


def test_pager_custom_command_with_args(shell):
    """Test setting a custom pager command with arguments"""
    test = (
        ShellTest(shell)
            .statement(".pager 'less -SR'")
            .statement('.pager')
    )
    result = test.run()
    result.check_stdout('less -SR')

def test_pager_with_query_output(shell):
    """Test that pager works with query output using cat"""
    test = (
        ShellTest(shell)
            .statement(".mode csv")
            .statement(".pager 'cat'")
            .statement('FROM range(10000)')
    )
    result = test.run()
    result.check_stdout('8888')

def test_pager_doesnt_affect_error_messages(shell):
    """Test that pager doesn't capture error messages"""
    test = (
        ShellTest(shell)
            .statement(".pager 'cat'")
            .statement(".mode csv")
            .statement('SELECT invalid_column FROM nonexistent_table')
    )
    result = test.run()
    result.check_stderr('Table')


def test_pager_preserves_nullvalue(shell):
    """Test that pager preserves null value rendering"""
    test = (
        ShellTest(shell)
            .statement('.nullvalue XYZ')
            .statement(".pager 'cat'")
            .statement('SELECT NULL FROM range(10000)')
    )
    result = test.run()
    result.check_stdout('XYZ')

def test_pager_multiple_queries(shell):
    """Test pager with multiple queries in sequence"""
    test = (
        ShellTest(shell)
            .statement(".pager 'cat'")
            .statement(".mode csv")
            .statement('SELECT 1 FROM range(1000)')
            .statement('SELECT 2 FROM range(1000)')
            .statement('SELECT 3 FROM range(1000)')
    )
    result = test.run()
    result.check_stdout('1')
    result.check_stdout('2')
    result.check_stdout('3')

def test_pager_small_data(shell):
    """Test pager with a small data set"""
    test = (
        ShellTest(shell)
            .statement(".pager 'unknown_cmd'")
            .statement(".mode csv")
            .statement('FROM range(10)')
    )
    result = test.run()
    result.check_stdout('9')


def test_duckbox_automatic_pager_uses_threshold(shell, tmp_path):
    pager_capture = tmp_path / "pager.out"
    pager_script = tmp_path / "pager.sh"
    pager_script.write_text(
        "#!/usr/bin/env sh\n"
        "set -eu\n"
        "cat > \"$DUCKDB_PAGER_CAPTURE\"\n"
        "printf 'PAGER_USED\\n'\n",
        encoding="utf-8",
    )
    os.chmod(pager_script, 0o755)

    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"
    env["DUCKDB_PAGER"] = str(pager_script)
    env["DUCKDB_PAGER_CAPTURE"] = str(pager_capture)

    out = _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=env,
        send_lines=[
            ".mode duckbox",
            ".pager set_row_threshold 2",
            "select i from range(5) t(i);",
            ".quit",
        ],
        send_after=["D ", "D ", "D ", "D "],
        timeout_s=15.0,
    )

    assert "PAGER_USED" in out
    captured = pager_capture.read_text(encoding="utf-8")
    assert "i" in captured
    assert "4" in captured

# fmt: on
