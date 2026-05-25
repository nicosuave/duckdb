# fmt: off

import pytest
import subprocess
import sys
from typing import List
from conftest import ShellTest
import os
from pathlib import Path

def test_dump_create(shell):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE a (i INTEGER);")
        .statement(".changes off")
        .statement("INSERT INTO a VALUES (42);")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout('CREATE TABLE a(i INTEGER)')
    result.check_stdout('COMMIT')

@pytest.mark.parametrize("pattern", [
    "a",
    "a%"
])
def test_dump_specific(shell, pattern):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE a (i INTEGER);")
        .statement(".changes off")
        .statement("INSERT INTO a VALUES (42);")
        .statement(f".dump {pattern}")
    )
    result = test.run()
    result.check_stdout('CREATE TABLE a(i INTEGER)')

# Original comment: more types, tables and views
def test_dump_mixed(shell):
    test = (
        ShellTest(shell)
        .statement("CREATE TABLE a (d DATE, k FLOAT, t TIMESTAMP);")
        .statement("CREATE TABLE b (c INTEGER);")
        .statement(".changes off")
        .statement("INSERT INTO a VALUES (DATE '1992-01-01', 0.3, NOW());")
        .statement("INSERT INTO b SELECT * FROM range(0,10);")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout('CREATE TABLE a(d DATE, k FLOAT, t TIMESTAMP);')

def test_dump_blobs(shell):
    test = (
        ShellTest(shell)
        .statement("create table test(t VARCHAR, b BLOB);")
        .statement(".changes off")
        .statement("insert into test values('literal blob', '\\x07\\x08\\x09');")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout("'\\x07\\x08\\x09'")

def test_dump_newline(shell):
    test = (
        ShellTest(shell)
        .statement("create table newline_data as select concat(chr(10), '\n') s;")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout("concat")
    result.check_stdout("chr(10)")

def test_dump_indexes(shell):
    test = (
        ShellTest(shell)
        .statement("create table integer(i int);")
        .statement("create index i_index on integer(i);")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout("CREATE INDEX i_index")

def test_dump_views(shell):
    test = (
        ShellTest(shell)
        .statement("create table integer(i int);")
        .statement("create view v1 as select * from integer;")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout("CREATE VIEW v1")

def test_dump_schema_qualified(shell):
    test = (
        ShellTest(shell)
        .statement("CREATE SCHEMA other;")
        .statement("CREATE TABLE other.t_in_other(a INT);")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout('CREATE SCHEMA IF NOT EXISTS other;')
    result.check_stdout('CREATE TABLE other.t_in_other(a INTEGER);')
    result.check_stdout('COMMIT')

def test_dump_schema_with_data(shell):
    test = (
        ShellTest(shell)
        .statement("CREATE SCHEMA test_schema;")
        .statement("CREATE TABLE test_schema.tbl(x INT, y VARCHAR);")
        .statement(".changes off")
        .statement("INSERT INTO test_schema.tbl VALUES (1, 'hello'), (2, 'world');")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout('CREATE SCHEMA IF NOT EXISTS test_schema;')
    result.check_stdout('CREATE TABLE test_schema.tbl(x INTEGER, y VARCHAR);')
    result.check_stdout("INSERT INTO test_schema.tbl VALUES(1,'hello');")
    result.check_stdout('COMMIT')

def test_dump_multiple_schemas(shell):
    test = (
        ShellTest(shell)
        .statement("CREATE SCHEMA s1;")
        .statement("CREATE SCHEMA s2;")
        .statement("CREATE TABLE s1.t1(a INT);")
        .statement("CREATE TABLE s2.t2(b INT);")
        .statement(".changes off")
        .statement("INSERT INTO s1.t1 VALUES (10);")
        .statement("INSERT INTO s2.t2 VALUES (20);")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout('CREATE SCHEMA IF NOT EXISTS s1;')
    result.check_stdout('CREATE SCHEMA IF NOT EXISTS s2;')
    result.check_stdout('INSERT INTO s1.t1 VALUES(10);')
    result.check_stdout('INSERT INTO s2.t2 VALUES(20);')

def test_dump_quoted_schema(shell):
    test = (
        ShellTest(shell)
        .statement('CREATE SCHEMA "my-schema";')
        .statement('CREATE TABLE "my-schema"."my-table"(a INT);')
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout('CREATE SCHEMA IF NOT EXISTS "my-schema";')
    result.check_stdout('CREATE TABLE "my-schema"."my-table"(a INTEGER);')

def test_dump_if_not_exists(shell):
    test = (
        ShellTest(shell)
        .statement("CREATE SCHEMA other;")
        .statement("CREATE TABLE IF NOT EXISTS other.tbl(x INT);")
        .statement(".changes off")
        .statement("INSERT INTO other.tbl VALUES (42);")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout('CREATE SCHEMA IF NOT EXISTS other;')
    result.check_stdout('INSERT INTO other.tbl VALUES(42);')
    result.check_stdout('COMMIT')


def test_dump_rejects_preserve_rowids(shell):
    result = ShellTest(shell).statement(".dump --preserve-rowids").run()
    assert result.status_code == 1
    result.check_stderr('Unknown option "--preserve-rowids" on ".dump"')


def test_dump_schema_semicolons_and_order(shell):
    test = (
        ShellTest(shell)
        .statement("CREATE SCHEMA s;")
        .statement("CREATE TABLE t(a INT, b VARCHAR);")
        .statement("CREATE TABLE s.u(x INT);")
        .statement("CREATE VIEW v AS SELECT 1 AS one;")
        .statement(".changes off")
        .statement("INSERT INTO t VALUES (1, 'x'), (2, NULL);")
        .statement("INSERT INTO s.u VALUES (42);")
        .statement(".dump")
    )
    result = test.run()
    assert result.status_code == 0

    out = result.stdout
    assert "CREATE SCHEMA IF NOT EXISTS s;;" in out

    idx_schema = out.find("CREATE SCHEMA IF NOT EXISTS s;;")
    idx_create_t = out.find("CREATE TABLE t(a INTEGER, b VARCHAR);;")
    idx_insert_t = out.find("INSERT INTO main.t VALUES(1,'x');")
    idx_create_u = out.find("CREATE TABLE s.u(x INTEGER);;")
    idx_insert_u = out.find("INSERT INTO s.u VALUES(42);")
    idx_view = out.find("CREATE VIEW v AS SELECT 1 AS one;;")
    idx_commit = out.rfind("COMMIT;")

    assert idx_schema != -1
    assert idx_create_t != -1
    assert idx_insert_t != -1
    assert idx_create_u != -1
    assert idx_insert_u != -1
    assert idx_view != -1
    assert idx_commit != -1

    assert idx_schema < idx_create_t < idx_insert_t < idx_create_u < idx_insert_u < idx_view < idx_commit


def test_dump_constraint_view_index_and_catalog_omissions(shell):
    test = (
        ShellTest(shell)
        .statement("CREATE SEQUENCE seq START 5;")
        .statement("CREATE TABLE t(i INTEGER DEFAULT nextval('seq'), j INTEGER CHECK(j > 0));")
        .statement("CREATE INDEX t_i_idx ON t(i);")
        .statement("CREATE MACRO add_one(x) AS x + 1;")
        .statement("CREATE VIEW v AS SELECT add_one(i) AS k FROM t;")
        .statement(".dump")
    )
    result = test.run()
    result.check_stdout("CHECK((j > 0))")
    result.check_stdout("CREATE INDEX t_i_idx")
    result.check_stdout("CREATE VIEW v AS SELECT add_one(i) AS k FROM t")
    result.check_not_exist("CREATE SEQUENCE")
    result.check_not_exist("CREATE MACRO")
