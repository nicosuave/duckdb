from conftest import ShellTest


def test_display_colors_prefix(shell):
    res = ShellTest(shell).statement(".display_colors").run()
    assert res.status_code == 0, res.stderr
    assert res.stdout.startswith("\x1b[38;5;52mdarkred1\x1b[00m \x1b[31mred\x1b[00m "), res.stdout
    plain = res.stdout.replace("\x1b[00m", "")
    import re
    plain = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", plain)
    tokens = plain.split()
    assert tokens[tokens.index("red3"):tokens.index("red3") + 4] == [
        "red3",
        "red4",
        "red1",
        "brightred",
    ]


def test_display_colors_respects_highlight_off(shell):
    res = (
        ShellTest(shell)
        .statement(".highlight off")
        .statement(".display_colors")
        .run()
    )
    assert res.status_code == 0, res.stderr
    assert "\x1b[" not in res.stdout
