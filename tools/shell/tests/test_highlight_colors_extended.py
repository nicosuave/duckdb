from conftest import ShellTest


def test_highlight_colors_extended_color_name(shell):
    res = ShellTest(shell).statement(".highlight_colors keyword blue1").run()
    assert res.status_code == 0


def test_highlight_colors_unknown_color_has_suggestions(shell):
    res = ShellTest(shell).statement(".highlight_colors keyword notacolor").run()
    assert res.status_code == 1
    res.check_stderr("Unknown highlighting color 'notacolor'")
    res.check_stderr('Did you mean: "tan"')
    res.check_stderr("Run '.display_colors' for a list of available colors.")

