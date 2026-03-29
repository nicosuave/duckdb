# fmt: off

from conftest import ShellTest


def test_edit_is_unsupported_in_batch(shell):
    result = ShellTest(shell).statement(".edit").run()
    assert result.status_code == 1
    result.check_stderr('Command "edit" is unsupported in the current version of the CLI')

