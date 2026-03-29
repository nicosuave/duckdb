# fmt: off

from conftest import ShellTest


def test_mode_column_unicode_width(shell):
    test = (
        ShellTest(shell)
        .statement(".mode column")
        .statement("select '漢字' as x, 'a' as y")
    )
    result = test.run()
    assert result.stdout == "x     y\n----  -\n漢字  a"


# fmt: on

