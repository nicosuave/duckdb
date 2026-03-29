# fmt: off

from conftest import ShellTest


def test_duckbox_truncation_dot_alignment(shell):
    result = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxrows 3")
        .statement("select range as x from range(15)")
        .run()
    )

    # Middle truncation markers are right-aligned in numeric columns.
    assert result.stdout.count("│         · │") == 3
    assert "│ ·         │" not in result.stdout


def test_duckbox_streaming_analyze_width_does_not_pad_for_unknown_row_count(shell):
    result = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxrows 3 1")
        .statement(".maxrows -1")
        .statement("select range as x from range(3)")
        .run()
    )

    # When max_analyze_rows is small we may render the header before row_count is known (streaming).
    # The shipped shell does not reserve footer width based on that placeholder row_count=0.
    result.check_stdout("┌───────┐\n│   x   │\n│ int64 │")
    assert "┌────────┐\n│   x    │\n│ int64  │" not in result.stdout

