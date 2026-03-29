import pytest

from conftest import ShellTest


@pytest.mark.parametrize(
    "cmd",
    [
        "keyword",
        "comment",
        "error",
    ],
)
def test_deprecated_highlight_color_alias_valid(shell, cmd):
    test = ShellTest(shell).statement(f".{cmd} brightred")
    result = test.run()
    assert result.status_code == 0
    result.check_stderr(
        f"WARNING: .{cmd} [COLOR] will be removed in a future release, use .render_color {cmd} brightred instead"
    )

@pytest.mark.parametrize(
    "cmd",
    [
        "constant",
        "cont",
        "cont_sel",
    ],
)
def test_deprecated_highlight_color_alias_invalid_element(shell, cmd):
    test = ShellTest(shell).statement(f".{cmd} brightred")
    result = test.run()
    assert result.status_code == 1
    result.check_stderr(
        f"WARNING: .{cmd} [COLOR] will be removed in a future release, use .render_color {cmd} brightred instead"
    )
    result.check_stderr(f"Unknown element '{cmd}', supported options: error, keyword")


def test_deprecated_highlight_color_alias_invalid(shell):
    test = ShellTest(shell).statement(".keyword bule")
    result = test.run()
    assert result.status_code == 1
    result.check_stderr(
        "WARNING: .keyword [COLOR] will be removed in a future release, use .render_color keyword bule instead"
    )
    result.check_stderr("Unknown highlighting color 'bule'")
    result.check_stderr('Did you mean: "blue", "blue1", "blue3", "blue4", "blueviolet"')
    result.check_stderr("Run '.display_colors' for a list of available colors.")
