# fmt: off

from conftest import ShellTest


def test_help_constant_not_listed(shell):
    result = ShellTest(shell).statement(".help constant").run()
    result.check_stdout("Nothing matches 'constant'")


def test_help_does_not_list_indices_alias(shell):
    result = ShellTest(shell).statement(".help").run()
    assert "indices" not in result.stdout
    result.check_stdout("indexes")
    result.check_stdout("Run .help --all for extended information")
    result.check_stdout("Run .help shortcuts for keyboard shortcuts")


def test_help_shortcuts(shell):
    result = ShellTest(shell).statement(".help shortcuts").run()
    assert result.status_code == 0, result.stderr
    assert "Control" in result.stdout
    assert "Editing" in result.stdout
    assert "Navigation" in result.stdout
    assert "History" in result.stdout
    assert "Enter / Ctrl+J" in result.stdout
    assert "Ctrl+C" in result.stdout
    assert "Ctrl+R" in result.stdout


def test_help_metadata_text_matches_1_5(shell):
    result = ShellTest(shell).statement(".help").run()
    assert ".bail on|off" in result.stdout
    assert "Default OFF" in result.stdout
    assert ".highlight_mode mixed|dark|light" in result.stdout
    assert ".highlight_mode mixed|dark|light|auto" not in result.stdout
    assert ".safe_mode" in result.stdout
    assert "Enable safe-mode" in result.stdout


def test_check_is_unknown_command(shell):
    result = ShellTest(shell).statement(".check").run()
    assert result.status_code == 1
    result.check_stderr("Unknown Command Error")
    assert "unsupported" not in result.stderr.lower()
