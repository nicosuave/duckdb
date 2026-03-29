# fmt: off

from conftest import ShellTest


def test_mode_invalid_keeps_prior_mode(shell):
    test = (
        ShellTest(shell)
        .statement(".mode csv")
        .statement(".mode semi")
        .statement("select 1 as a")
    )
    result = test.run()
    assert result.status_code == 1
    assert result.stderr == (
        "Error: mode should be one of: ascii box column csv duckbox html insert json jsonlines latex line list markdown quote table tabs tcl trash"
    )
    assert result.stdout == "a\r\n1"


# fmt: on
