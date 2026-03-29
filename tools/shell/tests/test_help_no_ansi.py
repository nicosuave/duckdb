from conftest import ShellTest


def test_help_has_no_ansi_sequences(shell):
    result = ShellTest(shell).statement(".help").run()
    assert result.status_code == 0, result.stderr
    assert "\x1b[" not in result.stdout

