# fmt: off

from conftest import ShellTest
from conftest import json_extension


def test_duckbox_struct_field_json_type(shell, json_extension):
    result = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement("select {'json': json('{\"a\": 1}')} as j;")
        .run()
    )
    result.check_stdout('struct("json" json)')
    assert 'struct("json" varchar)' not in result.stdout

