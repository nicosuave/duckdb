# fmt: off

from conftest import ShellTest


def test_echo_multistatement_tail_behavior(shell, tmp_path):
    sql = (
        "  select 1 as a; select 'x; y' as s; /* c; */ select $$dollar;semi$$ as d;\n"
        "select 2 as b; -- trailing\n"
    )
    path = tmp_path / "echo_tail.sql"
    path.write_text(sql, encoding="utf-8")

    result = ShellTest(shell, ["-echo", "-f", str(path)]).run()

    # Match shipped shell echo behavior for multi-statement lines (quirky prefix slices).
    result.check_stdout("select 1 as a\n")
    result.check_stdout("select 1 as a; sele\n")
    result.check_stdout("select 1 as a; select 'x; y' as s; /*")
    result.check_stdout("select 2 as b; -- trailing")
