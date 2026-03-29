from conftest import ShellTest


def test_last_renders_untruncated(shell):
    test = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxrows 1")
        .statement("select i from range(10) t(i)")
        .statement(".last")
    )
    result = test.run()
    result.check_stdout("(1 shown)")
    result.check_stdout("│     9 │")

