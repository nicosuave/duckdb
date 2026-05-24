# fmt: off

from conftest import ShellTest


def test_duckbox_unicode_width_zwj(shell):
    test = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement("select 'a' as a, '∑' as sigma, '👩‍💻' as emoji")
    )
    result = test.run()
    result.check_stdout("│ a       │ ∑       │ 👩‍💻      │")
