from conftest import ShellTest


def test_help_has_ansi_sequences(shell):
    result = ShellTest(shell).statement(".help").run()
    assert result.status_code == 0, result.stderr
    assert "\x1b[32m.bail\x1b[00m\x1b[33m on|off\x1b[00m" in result.stdout


def test_help_has_no_ansi_sequences_when_highlighting_is_off(shell):
    result = ShellTest(shell).statement(".highlight off").statement(".help").run()
    assert result.status_code == 0, result.stderr
    assert "\x1b[32m.bail" not in result.stdout
    assert ".bail on|off" in result.stdout
