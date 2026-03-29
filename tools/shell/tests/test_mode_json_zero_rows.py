# fmt: off

import pytest
from conftest import ShellTest


@pytest.mark.parametrize("mode", ["json", "jsonlines"])
def test_mode_json_zero_rows_prints_nothing(shell, mode):
    test = (
        ShellTest(shell)
        .statement(f".mode {mode}")
        .statement("select i from range(0) tbl(i)")
    )
    result = test.run()
    assert result.stdout == ""

