# fmt: off

import subprocess


def run_list_mode(shell: str, sql: str):
    res = subprocess.run(
        [shell, "--batch", "--init", "/dev/null", "-list", ":memory:"],
        input=sql.encode("utf-8"),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return res.returncode, res.stdout.decode("utf-8").strip(), res.stderr.decode("utf-8").strip()


def test_statement_splitting_semicolon_in_string(shell):
    code, out, err = run_list_mode(shell, "select 'a; b' as s;\nselect 42 as x;\n")
    assert code == 0, err
    assert out == "s\na; b\nx\n42"


def test_statement_splitting_semicolon_in_dollar_quoted_string(shell):
    code, out, err = run_list_mode(shell, "select $$a; b$$ as s;\nselect 42 as x;\n")
    assert code == 0, err
    assert out == "s\na; b\nx\n42"


def test_statement_splitting_semicolon_in_line_comment(shell):
    code, out, err = run_list_mode(shell, "select 1 as a; -- comment ;\nselect 2 as b;\n")
    assert code == 0, err
    assert out == "a\n1\nb\n2"


def test_statement_splitting_semicolon_in_block_comment(shell):
    code, out, err = run_list_mode(shell, "select 1 /* comment ; */ as a;\nselect 2 as b;\n")
    assert code == 0, err
    assert out == "a\n1\nb\n2"


# fmt: on

