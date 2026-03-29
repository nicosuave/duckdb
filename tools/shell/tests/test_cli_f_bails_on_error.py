# fmt: off

from conftest import ShellTest


def test_f_bails_on_error(tmp_path, shell):
    script = tmp_path / "script.sql"
    script.write_text("select * from;\nselect 42;\n")

    result = ShellTest(shell, ["-f", script.as_posix()]).run()
    assert result.status_code != 0
    assert "42" not in result.stdout

