# fmt: off

import os

from test_interactive_startup import _pty_run


def _env(tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "dumb"
    return env


def _normalize(out: str) -> str:
    # PTY output often includes CRLF, and when the process explicitly prints CRLF
    # (e.g. .mode csv uses "\r\n"), the PTY can translate the '\n' again and we end
    # up with "\r\r\n". Normalize this so tests stay stable.
    out = out.replace("\r\r\n", "\n")
    out = out.replace("\r\n", "\n")
    out = out.replace("\r", "")
    return out


def test_interactive_semicolon_in_string_does_not_terminate(shell, tmp_path):
    out = _normalize(
        _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=_env(tmp_path),
        send_lines=[
            ".mode csv",
            "select ';' as s",
            ";",
            ".quit",
        ],
        send_after=["D ", "D ", "· ", "D "],
        timeout_s=10.0,
    )
    )

    assert "· " in out
    assert "\ns\n;\n" in out


def test_interactive_semicolon_in_comment_does_not_terminate(shell, tmp_path):
    out = _normalize(
        _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=_env(tmp_path),
        send_lines=[
            ".mode csv",
            "select 1 -- ;",
            ";",
            ".quit",
        ],
        send_after=["D ", "D ", "· ", "D "],
        timeout_s=10.0,
    )
    )

    assert "· " in out
    # Default CSV behavior includes a header row; "select 1" produces header "1" then value "1".
    assert "\n1\n1\n" in out


def test_interactive_multiple_statements_single_line(shell, tmp_path):
    out = _normalize(
        _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=_env(tmp_path),
        send_lines=[
            ".mode csv",
            "select 1 as a; select 2 as b;",
            ".quit",
        ],
        send_after=["D ", "D ", "D "],
        timeout_s=10.0,
    )
    )

    assert "\na\n1\nb\n2\n" in out


def test_interactive_dollar_quoted_semicolon_does_not_terminate(shell, tmp_path):
    out = _normalize(
        _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=_env(tmp_path),
        send_lines=[
            ".mode csv",
            "select $$;$$ as s",
            ";",
            ".quit",
        ],
        send_after=["D ", "D ", "· ", "D "],
        timeout_s=10.0,
    )
    )

    assert "· " in out
    assert "\ns\n;\n" in out


def test_interactive_semicolon_before_unterminated_block_comment_executes_after_close(shell, tmp_path):
    out = _normalize(
        _pty_run(
        shell=shell,
        args=["-interactive", "--init", "/dev/null"],
        env=_env(tmp_path),
        send_lines=[
            ".mode csv",
            "select 1; /*",
            "*/",
            ".quit",
        ],
        send_after=["D ", "D ", "· ", "D "],
        timeout_s=10.0,
    )
    )

    assert "· " in out
    assert "\n1\n1\n" in out
