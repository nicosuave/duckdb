# fmt: off

from conftest import ShellTest


def test_mode_json_prints_trailing_newline(shell):
    test = (
        ShellTest(shell)
        .statement(".mode json")
        .statement("select 1 as a, 'x' as b;")
        .statement(".mode jsonlines")
        .statement("select 1 as a, 'x' as b;")
        .statement(".mode csv")
        .statement("select 1 as a, 'x' as b;")
    )
    result = test.run()
    result.check_stdout("]\n{\"a\":1,\"b\":\"x\"}\na,b")

