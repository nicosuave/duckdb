# fmt: off

import pytest
from conftest import ShellTest


EXPECTED = {
    "csv": "c,q,s,n\r\n\"a,b\",\"a\"\"b\",\"a'b\",NULL",
    "tabs": "c\tq\ts\tn\na,b\ta\"b\ta'b\tNULL",
    "list": "c|q|s|n\na,b|a\"b|a'b|NULL",
    "quote": "'c','q','s','n'\n'a,b','a\"b','a''b',NULL",
    "json": "[{\"c\":\"a,b\",\"q\":\"a\\\"b\",\"s\":\"a'b\",\"n\":null}]",
    "jsonlines": "{\"c\":\"a,b\",\"q\":\"a\\\"b\",\"s\":\"a'b\",\"n\":null}",
    "insert": "INSERT INTO \"table\"(c,q,s,n) VALUES('a,b','a\"b','a''b',NULL);",
    "tcl": "\"c\" \"q\" \"s\" \"n\"\n\"a,b\" \"a\\\"b\" \"a'b\" \"NULL\"",
}


@pytest.mark.parametrize("mode", list(EXPECTED.keys()))
def test_mode_exact_quoting(shell, mode):
    test = (
        ShellTest(shell)
        .statement(f".mode {mode}")
        .statement("select 'a,b' as c, 'a\"b' as q, 'a''b' as s, NULL as n")
    )
    result = test.run()
    assert result.stdout == EXPECTED[mode]


# fmt: on

