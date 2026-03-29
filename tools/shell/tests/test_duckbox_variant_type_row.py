# fmt: off

from conftest import ShellTest


def test_duckbox_variant_type_row(shell):
    test = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement("select ' {\"a\":1,\"b\":[2,3] } '::variant as v")
    )
    result = test.run()
    result.check_stdout("│       variant        │")
