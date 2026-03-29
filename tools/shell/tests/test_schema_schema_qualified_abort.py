from pathlib import Path

from conftest import ShellTest


def test_schema_schema_qualified_pattern_bails(shell, tmp_path: Path):
    script = tmp_path / "schema_qualified.sql"
    script.write_text(
        "\n".join(
            [
                "create schema s;",
                "create table s.t(a int, b varchar);",
                "create index idx on s.t(a);",
                "create view s.v as select a from s.t;",
                ".schema",
                ".schema s.%",
                ".schema --indent",
            ]
        )
        + "\n"
    )

    result = ShellTest(shell).add_argument("-f", str(script)).run()
    assert result.status_code == 1

    result.check_stderr('Referenced column "sname" not found')
    result.check_stderr("Error: querying schema information")

    assert "CREATE TABLE s.t(a INTEGER, b VARCHAR);;" not in result.stdout
