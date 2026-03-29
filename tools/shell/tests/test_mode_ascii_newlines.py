# fmt: off

from conftest import ShellTest


def test_mode_ascii_uses_newlines(shell):
    test = (
        ShellTest(shell)
        .statement(".mode ascii")
        .statement("select 1 as a, 'x' as b;")
    )
    result = test.run()
    result.check_stdout("a\nb\n1\nx")
    result.check_not_exist("\x1f")
    result.check_not_exist("\x1e")

