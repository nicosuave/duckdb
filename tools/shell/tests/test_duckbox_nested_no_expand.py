# fmt: off

from conftest import ShellTest


def test_duckbox_nested_values_do_not_expand_when_they_fit(shell):
    test = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxwidth 40")
        .statement(".maxrows 40")
        .statement("select [1,2,3,4,5] as l, {'a': 1, 'b': [2,3], 'c': {'d': 4}} as s")
    )
    result = test.run()
    result.check_stdout("[1, 2, 3, 4, 5]")
    result.check_stdout("{'a': 1, 'b': [2, 3], 'c': {'d': 4}}")
    result.check_not_exist("│ [               │ {")
    result.check_not_exist("│   1, 2, 3, 4, 5 │")


# fmt: on
