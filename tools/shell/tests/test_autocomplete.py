# fmt: off

import pytest
import subprocess
import sys
from typing import List
from conftest import ShellTest
from conftest import autocomplete_extension
import os
import pty
import select
import time
import re


def _strip_ansi(s: str) -> str:
    s = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", s)
    s = re.sub(r"\x1b\][^\x07]*\x07", "", s)
    return s


def _read_until(fd: int, pid: int, needle: str, timeout_s: float = 10.0) -> str:
    buf = bytearray()
    recent = bytearray()
    deadline = time.time() + timeout_s

    while True:
        if time.time() >= deadline:
            raise AssertionError(
                f"timeout waiting for {needle!r}, output so far:\n{buf.decode('utf-8', errors='ignore')}"
            )
        exited_pid, status = os.waitpid(pid, os.WNOHANG)
        if exited_pid != 0:
            raise AssertionError(
                f"process exited while waiting for {needle!r} (status={status}), output so far:\n{buf.decode('utf-8', errors='ignore')}"
            )

        r, _, _ = select.select([fd], [], [], 0.1)
        if not r:
            continue
        chunk = os.read(fd, 4096)
        if not chunk:
            continue
        buf.extend(chunk)
        recent.extend(chunk)
        if len(recent) > 256:
            del recent[:-256]
        if b"\x1b[6n" in recent:
            os.write(fd, b"\x1b[1;1R")
            recent = recent.replace(b"\x1b[6n", b"")

        text = _strip_ansi(buf.decode("utf-8", errors="ignore"))
        if needle in text:
            return text

# 'autocomplete_extension' is a fixture which will skip the test if 'autocomplete' is not loaded
def test_autocomplete_select(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CALL sql_auto_complete('SEL')")
    )
    result = test.run()
    result.check_stdout('SELECT')


def test_interactive_autocomplete_extra_char(shell, autocomplete_extension, tmp_path):
    env = os.environ.copy()
    env["HOME"] = str(tmp_path)
    env["DUCKDB_HISTORY"] = str(tmp_path / ".duckdb_history")
    env["TERM"] = "xterm-256color"
    env["COLUMNS"] = "80"
    env["ROWS"] = "24"

    pid, master_fd = pty.fork()
    if pid == 0:
        os.execvpe(shell, [shell, "-interactive", "--init", "/dev/null"], env)

    try:
        _read_until(master_fd, pid, "D ")
        os.write(master_fd, b".mode csv\r")
        _read_until(master_fd, pid, "D ")
        os.write(master_fd, b"SEL")
        _read_until(master_fd, pid, "SEL")
        os.write(master_fd, b"\t")
        _read_until(master_fd, pid, "SELECT")
        os.write(master_fd, b" 77 as v;\r")
        out = _read_until(master_fd, pid, "77")
        assert "SELECT 77 as v;" in out
        assert "SELECT  77 as v;" not in out
        os.write(master_fd, b".quit\r")
    finally:
        try:
            os.close(master_fd)
        except OSError:
            pass

def test_autocomplete_first_from(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CALL sql_auto_complete('FRO')")
    )
    result = test.run()
    result.check_stdout('FROM')

def test_autocomplete_column(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('SELECT my_') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('my_column')

def test_autocomplete_where(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('SELECT my_column FROM my_table WH') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('WHERE')

def test_autocomplete_insert(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('INS') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('INSERT')

def test_autocomplete_into(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('INSERT IN') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('INTO')

def test_autocomplete_into_table(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('INSERT INTO my_t') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('my_table')

def test_autocomplete_values(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('INSERT INTO my_table VAL') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('VALUES')

def test_autocomplete_delete(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('DEL') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('DELETE')

def test_autocomplete_delete_from(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('DELETE F') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('FROM')

def test_autocomplete_from_table(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('DELETE FROM m') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('my_table')

def test_autocomplete_update(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('UP') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('UPDATE')

def test_autocomplete_update_table(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('UPDATE m') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('my_table')

    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("""SELECT * FROM sql_auto_complete('UPDATE "m') LIMIT 1;""")
    )
    result = test.run()
    result.check_stdout('my_table')

def test_autocomplete_update_column(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE my_table(my_column INTEGER);")
        .statement("SELECT * FROM sql_auto_complete('UPDATE my_table SET m') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('my_column')

def test_autocomplete_funky_table(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("""CREATE TABLE "Funky Table With Spaces"(my_column INTEGER);""")
        .statement("SELECT suggestion FROM sql_auto_complete('SELECT * FROM F') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('"Funky Table With Spaces"')

    test = (
        ShellTest(shell)
        .statement("""CREATE TABLE "Funky Table With Spaces"("Funky Column" int);""")
        .statement("""SELECT suggestion FROM sql_auto_complete('select "Funky Column" FROM f') LIMIT 1;""")
    )
    result = test.run()
    result.check_stdout('"Funky Table With Spaces"')

def test_autocomplete_funky_column(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("""CREATE TABLE "Funky Table With Spaces"("Funky Column" int);""")
        .statement("SELECT * FROM sql_auto_complete('select f') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('"Funky Column"')

def test_autocomplete_semicolon(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("SELECT * FROM sql_auto_complete('SELECT 42; SEL') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('SELECT')

def test_autocomplete_comments(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("""
SELECT * FROM sql_auto_complete('--SELECT * FROM
SEL') LIMIT 1;""")
    )
    result = test.run()
    result.check_stdout('SELECT')

def test_autocomplete_scalar_functions(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("SELECT * FROM sql_auto_complete('SELECT regexp_m') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('regexp_matches')

def test_autocomplete_aggregates(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("SELECT * FROM sql_auto_complete('SELECT approx_c') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('approx_count_distinct')

def test_autocomplete_builtin_views(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("SELECT * FROM sql_auto_complete('SELECT * FROM sqlite_ma') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('sqlite_master')

def test_autocomplete_table_function(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("SELECT * FROM sql_auto_complete('SELECT * FROM read_csv_a') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('read_csv_auto')

def test_autocomplete_tpch(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE partsupp(ps_suppkey int);")
        .statement("CREATE TABLE supplier(s_suppkey int);")
        .statement("CREATE TABLE nation(n_nationkey int);")
        .statement("SELECT * FROM sql_auto_complete('DROP TABLE na') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('nation')

    test = (
        ShellTest(shell)
        .statement("CREATE TABLE partsupp(ps_suppkey int);")
        .statement("CREATE TABLE supplier(s_suppkey int);")
        .statement("CREATE TABLE nation(n_nationkey int);")
        .statement("SELECT * FROM sql_auto_complete('SELECT s_supp') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('s_suppkey')

    test = (
        ShellTest(shell)
        .statement("CREATE TABLE partsupp(ps_suppkey int);")
        .statement("CREATE TABLE supplier(s_suppkey int);")
        .statement("CREATE TABLE nation(n_nationkey int);")
        .statement("SELECT * FROM sql_auto_complete('SELECT * FROM partsupp JOIN supp') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('supplier')

    test = (
        ShellTest(shell)
        .statement("CREATE TABLE partsupp(ps_suppkey int);")
        .statement("CREATE TABLE supplier(s_suppkey int);")
        .statement("CREATE TABLE nation(n_nationkey int);")
        .statement(".mode csv")
        .statement("SELECT l,l FROM sql_auto_complete('SELECT * FROM partsupp JOIN supplier ON (s_supp') t(l) LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('s_suppkey,s_suppkey')

    test = (
        ShellTest(shell)
        .statement("CREATE TABLE partsupp(ps_suppkey int);")
        .statement("CREATE TABLE supplier(s_suppkey int);")
        .statement("CREATE TABLE nation(n_nationkey int);")
        .statement("SELECT * FROM sql_auto_complete('SELECT * FROM partsupp JOIN supplier USING (ps_su') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('ps_suppkey')

def test_autocomplete_from(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("SELECT * FROM sql_auto_complete('SELECT * FR') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('FROM')

def test_autocomplete_disambiguation_column(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE MyTable(MyColumn Varchar);")
        .statement("SELECT * FROM sql_auto_complete('SELECT My') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('MyColumn')
    
def test_autocomplete_disambiguation_table(shell, autocomplete_extension):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE MyTable(MyColumn Varchar);")
        .statement("SELECT * FROM sql_auto_complete('SELECT MyColumn FROM My') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout('MyTable')

def test_autocomplete_directory(shell, autocomplete_extension, tmp_path):
    shell_test_dir = tmp_path / 'shell_test_dir'
    extra_path = tmp_path / 'shell_test_dir' / 'extra_path'
    shell_test_dir.mkdir()
    extra_path.mkdir()

    # Create the files
    base_files = ['extra.parquet', 'extra.file']
    for fname in base_files:
        with open(shell_test_dir / fname, 'w+') as f:
            f.write('')

    # Complete the directory
    partial_directory = tmp_path / 'shell_test'
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE MyTable(MyColumn Varchar);")
        .statement(f"SELECT * FROM sql_auto_complete('SELECT * FROM ''{partial_directory.as_posix()}') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout("shell_test_dir")

    # Complete the sub directory as well
    partial_subdirectory = tmp_path / 'shell_test_dir' / 'extra'
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE MyTable(MyColumn Varchar);")
        .statement(f"SELECT * FROM sql_auto_complete('SELECT * FROM ''{partial_subdirectory.as_posix()}') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout("extra_path")

    # Complete the parquet file in the sub directory
    partial_parquet = tmp_path / 'shell_test_dir' / 'extra.par'
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE MyTable(MyColumn Varchar);")
        .statement(f"SELECT * FROM sql_auto_complete('SELECT * FROM ''{partial_parquet.as_posix()}') LIMIT 1;")
    )
    result = test.run()
    result.check_stdout("extra.parquet")

# fmt: on
