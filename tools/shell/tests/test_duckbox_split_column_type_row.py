# fmt: off

from conftest import ShellTest


def test_duckbox_split_column_type_row_shows_ellipsis(shell):
    res = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxwidth 80")
        .statement(
            "select "
            "chr(7) || ' bell' as bell, "
            "chr(8) || ' bs' as bs, "
            "chr(9) || ' tab' as tab, "
            "chr(10) || ' nl' as nl, "
            "chr(11) || ' vt' as vt, "
            "chr(12) || ' ff' as ff, "
            "chr(13) || ' cr' as cr, "
            "chr(27) || ' esc' as esc, "
            "chr(1) || ' soh' as soh, "
            "chr(0) || ' nul' as nul"
        )
        .run()
    )

    assert "│ varchar │ varchar │ varchar │ varchar │ … │ varchar │ varchar │ varchar │" in res.stdout
    assert "│ varchar │ varchar │ varchar │ varchar │   │ varchar │ varchar │ varchar │" not in res.stdout


# fmt: on

