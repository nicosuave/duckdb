# fmt: off

import pytest
from conftest import ShellTest


@pytest.mark.parametrize(
    "mode, expected",
    [
        ("csv", "s\r\n\"a\nb\""),
        ("quote", "'s'\n'a\nb'"),
        ("insert", "INSERT INTO \"table\"(s) VALUES(concat('a', chr(10), 'b'));"),
    ],
)
def test_mode_newline_value_rendering(shell, mode, expected):
    test = (
        ShellTest(shell)
        .statement(f".mode {mode}")
        .statement("select 'a'||chr(10)||'b' as s")
    )
    result = test.run()
    assert result.stdout == expected


# fmt: on

