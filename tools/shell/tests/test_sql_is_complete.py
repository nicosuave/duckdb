# fmt: off

from conftest import ShellTest


def test_dollar_quoted_string_semicolon_newline(shell):
    test = (
        ShellTest(shell)
        .statement(".mode box")
        .statement("select $$hello;\nworld$$ as s")
    )
    result = test.run()
    result.check_stdout(r"hello;\nworld")


def test_dollar_quoted_string_tagged(shell):
    test = (
        ShellTest(shell)
        .statement(".mode box")
        .statement("select $tag123$hi;there$tag123$ as s")
    )
    result = test.run()
    result.check_stdout("hi;there")
