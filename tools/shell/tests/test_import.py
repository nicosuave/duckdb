# fmt: off

import pytest
import subprocess
import sys
from typing import List
from conftest import ShellTest
import os
from pathlib import Path


# test import from a parquet file
def test_import_parquet(shell):
    test = (
        ShellTest(shell)
        .statement(f'.import data/parquet-testing/unsigned.parquet a')
        .statement("SELECT MAX(d) FROM a")
    )

    result = test.run()
    result.check_stdout("18446744073709551615")

# test import from a json file
def test_import_json(shell):
    test = (
        ShellTest(shell)
        .statement(f'.import data/json/example_n.ndjson a')
        .statement("SELECT name FROM a")
    )

    result = test.run()
    result.check_stdout("Broadcast News")


def test_import_dotted_table_name_appends_like_official(shell, tmp_path):
    file = tmp_path / "data.csv"
    file.write_text("i\n1\n2\n", encoding="utf-8")

    test = (
        ShellTest(shell)
        .statement("CREATE SCHEMA s")
        .statement(f".import --csv --header true {file.as_posix()} s.t")
        .statement(f".import --csv --header true {file.as_posix()} s.t")
        .statement('SELECT count(*) FROM "s.t"')
    )

    result = test.run()
    result.check_stdout("4")


def test_import_csv_generic_parameters(shell, tmp_path):
    file = tmp_path / "pipe.csv"
    file.write_text("a|b\n1|x\n2|y\n", encoding="utf-8")

    test = (
        ShellTest(shell)
        .statement(f".import --csv --delim | --header true {file.as_posix()} pipe_tbl")
        .statement("SELECT sum(a), count(*) FROM pipe_tbl")
    )

    result = test.run()
    result.check_stdout("3")
    result.check_stdout("2")


# fmt: on
