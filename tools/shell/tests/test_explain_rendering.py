# fmt: off

from conftest import ShellTest


def test_explain_uses_explain_renderer(shell):
    test = (
        ShellTest(shell)
        .statement(".mode box")
        .statement("explain select 1")
    )
    result = test.run()
    # Explain output should use the dedicated EXPLAIN renderer (not a regular 2-column table).
    result.check_stdout("┌─────────────────────────────┐")
    result.check_stdout("Physical Plan")

