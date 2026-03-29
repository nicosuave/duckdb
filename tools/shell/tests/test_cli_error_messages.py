# fmt: off

from conftest import ShellTest


def test_unknown_option_did_you_mean(shell):
    result = ShellTest(shell, ["-hepl"]).run()
    assert result.status_code != 0
    result.check_stderr("Unknown Option Error")
    result.check_stderr("Did you mean")
    result.check_stderr("-help")


def test_unknown_command_did_you_mean(shell):
    result = ShellTest(shell).statement(".hep").run()
    assert result.status_code != 0
    result.check_stderr("Unknown Command Error")
    result.check_stderr("Did you mean")
    result.check_stderr(".help")

