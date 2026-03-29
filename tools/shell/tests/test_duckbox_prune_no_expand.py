# fmt: off

from conftest import ShellTest


def test_duckbox_no_expand_when_columns_pruned(shell):
    res = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxwidth 60")
        .statement(".maxrows 40")
        .statement(
            "select [1,2,3,4,5,6,7,8,9,10] as l, repeat('x',30) as a, repeat('y',30) as b, repeat('z',30) as c, repeat('w',30) as d, repeat('v',30) as e, repeat('u',30) as f"
        )
        .run()
    )
    res.check_stdout(
        """┌────────────────────────────┬───┬──────────────────────┬──────────────────────┐
│             l              │ … │          e           │          f           │
│          int32[]           │ … │       varchar        │       varchar        │
├────────────────────────────┼───┼──────────────────────┼──────────────────────┤
│ [1, 2, 3, 4, 5, 6, 7, 8, … │ … │ vvvvvvvvvvvvvvvvvvv… │ uuuuuuuuuuuuuuuuuuu… │
├────────────────────────────┴───┴──────────────────────┴──────────────────────┤
│ 1 rows                                                   7 columns (3 shown) │
└──────────────────────────────────────────────────────────────────────────────┘"""
    )
    res.check_stderr(None)


# fmt: on

