# fmt: off

from conftest import ShellTest


def test_mode_html_escapes_lt_gt_amp(shell):
    test = (
        ShellTest(shell)
        .statement(".mode html")
        .statement("select '<tag>&' as x")
    )
    result = test.run()
    assert result.stdout == "<tr><th>x</th>\n</tr>\n<tr><td>&lt;tag&gt;&amp;</td>\n</tr>"


# fmt: on

