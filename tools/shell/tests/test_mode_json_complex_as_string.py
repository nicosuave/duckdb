# fmt: off

from conftest import ShellTest


def test_mode_json_renders_struct_natively(shell):
    test = (
        ShellTest(shell)
        .statement(".mode json")
        .statement("select {'a': 1, 'b': [2,3]} as st")
    )
    result = test.run()
    result.check_stdout('"st":{"a":1,"b":[2,3]}')


def test_mode_json_renders_array_natively(shell):
    test = (
        ShellTest(shell)
        .statement(".mode json")
        .statement("select [1,2,3]::integer[] as arr")
    )
    result = test.run()
    result.check_stdout('"arr":[1,2,3]')
