# fmt: off

from conftest import ShellTest


def test_mode_box_unicode_width(shell):
    test = (
        ShellTest(shell)
        .statement(".mode box")
        .statement("select '漢字' as x")
    )
    result = test.run()
    assert result.stdout == "┌──────┐\n│  x   │\n├──────┤\n│ 漢字 │\n└──────┘"


# fmt: on

