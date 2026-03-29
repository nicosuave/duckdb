# fmt: off

from conftest import ShellTest


def test_struct_key_with_single_quote_renders_shell_style(shell):
    test = (
        ShellTest(shell)
        .statement(".mode list")
        .statement(".headers off")
        .statement("select {'a''b': 1} as s")
    )
    result = test.run()
    assert result.stdout == "{'a\\'b': 1}"

