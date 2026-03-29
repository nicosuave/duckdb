from conftest import ShellTest


def test_highlight_mode_auto(shell):
    res = ShellTest(shell).statement(".highlight_mode auto").run()
    assert res.status_code == 0
