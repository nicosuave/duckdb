# fmt: off

import pytest
from conftest import ShellTest


@pytest.mark.parametrize(
    "mode, expected",
    [
        ("json", '[{"j":{"a":1}}]'),
        ("jsonlines", '{"j":{"a":1}}'),
    ],
)
def test_mode_json_renders_typed_json(shell, mode, expected):
    test = (
        ShellTest(shell)
        .statement(f".mode {mode}")
        .statement("select '{\"a\":1}'::json as j")
    )
    result = test.run()
    assert result.stdout == expected


# fmt: on

