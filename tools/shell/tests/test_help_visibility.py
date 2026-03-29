# fmt: off

from conftest import ShellTest


def test_help_constant_not_listed(shell):
    result = ShellTest(shell).statement(".help constant").run()
    result.check_stdout("Nothing matches 'constant'")


def test_help_does_not_list_indices_alias(shell):
    result = ShellTest(shell).statement(".help").run()
    assert "indices" not in result.stdout
    result.check_stdout("indexes")
    result.check_stdout("Run .help --all for extended information")


def test_check_is_unknown_command(shell):
    result = ShellTest(shell).statement(".check").run()
    assert result.status_code == 1
    result.check_stderr("Unknown Command Error")
    assert "unsupported" not in result.stderr.lower()
