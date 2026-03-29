# fmt: off

import pytest
from conftest import ShellTest


EXPECTED = {
    "ascii": "a\nb\n1\nx",
    "box": "┌───┬───┐\n│ a │ b │\n├───┼───┤\n│ 1 │ x │\n└───┴───┘",
    "column": "a  b\n-  -\n1  x",
    "csv": "a,b\r\n1,x",
    "duckbox": "┌───────┬─────────┐\n│   a   │    b    │\n│ int32 │ varchar │\n├───────┼─────────┤\n│     1 │ x       │\n└───────┴─────────┘",
    "html": "<tr><th>a</th>\n<th>b</th>\n</tr>\n<tr><td>1</td>\n<td>x</td>\n</tr>",
    "insert": "INSERT INTO \"table\"(a,b) VALUES(1,'x');",
    "json": '[{"a":1,"b":"x"}]',
    "jsonlines": '{"a":1,"b":"x"}',
    "latex": "\\begin{tabular}{|rl|}\n\\hline\na & b \\\\\n\\hline\n1 & x \\\\\n\\hline\n\\end{tabular}",
    "line": "a = 1\n    b = x",
    "list": "a|b\n1|x",
    "markdown": "| a | b |\n|--:|---|\n| 1 | x |",
    "quote": "'a','b'\n1,'x'",
    "table": "+---+---+\n| a | b |\n+---+---+\n| 1 | x |\n+---+---+",
    "tabs": "a\tb\n1\tx",
    "tcl": "\"a\" \"b\"\n\"1\" \"x\"",
    "trash": "",
}


@pytest.mark.parametrize("mode", list(EXPECTED.keys()))
def test_mode_exact_small(shell, mode):
    test = (
        ShellTest(shell)
        .statement(f".mode {mode}")
        .statement("select 1 as a, 'x' as b")
    )
    result = test.run()
    assert result.stdout == EXPECTED[mode]


# fmt: on
