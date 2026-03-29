# fmt: off

from conftest import ShellTest


def test_mode_markdown_escapes_pipes(shell):
    test = (
        ShellTest(shell)
        .statement(".mode markdown")
        .statement("select 'a|b' as x")
    )
    result = test.run()
    assert result.stdout == "|  x   |\n|------|\n| a\\|b |"


def test_mode_markdown_escapes_pipes_multi_column(shell):
    test = (
        ShellTest(shell)
        .statement(".mode markdown")
        .statement("select 'a|b' as s, 'c' as t")
    )
    result = test.run()
    assert result.stdout == "|  s   | t |\n|------|---|\n| a\\|b | c |"


# fmt: on
