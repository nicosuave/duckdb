# fmt: off

import pytest
import subprocess
import sys
from typing import List
from conftest import ShellTest
import os

def test_readable_numbers(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering footer")
        .statement("select 59986052 as count, 123456789 as count2, 9999999999 count3, -9999999999 count4;")
    )
    result = test.run()
    result.check_stdout("(59.99 million)")
    result.check_stdout("(123.46 million)")
    result.check_stdout("(10.00 billion)")
    result.check_stdout("(-10.00 billion)")

@pytest.mark.parametrize('test_rounding', [False, True])
def test_readable_numbers_exhaustive(shell, test_rounding):
    query = "select "
    for i in range(1, 20):
        if i > 1:
            query += ", "
        if test_rounding:
            query += '9' * i
        else:
            query += '1' + ('0' * i)
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering all")
        .statement(".maxwidth 99999")
        .statement(query)
    )
    result = test.run()
    for unit in ['million', 'billion', 'trillion', 'quadrillion', 'quintillion']:
        for number in ['1.00', '10.00', '100.00']:
            if unit == 'quintillion' and number in ['10.00', '100.00']:
                continue
            result.check_stdout(number + " " + unit)

def test_readable_numbers_rounding(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering footer")
        .statement(".maxwidth 99999")
        .statement("select 1005000, 1004999, -1005000, -1004999;")
    )
    result = test.run()
    result.check_stdout("(1.01 million)")
    result.check_stdout("(1.00 million)")
    result.check_stdout("(-1.01 million)")
    result.check_stdout("(-1.00 million)")

def test_readable_rounding_edge_case(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering all")
        .statement(".maxwidth 99999")
        .statement("select 994999, 995000")
    )
    result = test.run()
    result.check_stdout("1.00 million")

def test_readable_numbers_limit(shell):
    test = (
        ShellTest(shell)
        .statement(".maxwidth 99999")
        .statement(".large_number_rendering all")
        .statement("select 18446744073709551616, -18446744073709551616, 9999999999999999999, -9999999999999999999;")
    )
    result = test.run()
    result.check_stdout("10.00 quintillion")
    result.check_stdout("-10.00 quintillion")

def test_decimal_separator(shell):
    test = (
        ShellTest(shell)
        .statement(".decimal_sep ,")
        .statement(".large_number_rendering all")
        .statement("select 59986052, 59986052.5, 999999999.123456789, 1e20, 'nan'::double;")
    )
    result = test.run()
    result.check_stdout("59,99 million")
    result.check_stdout("1,00 billion")

def test_decimal_and_thousand_separator(shell):
    test = (
        ShellTest(shell)
        .statement(".decimal_sep ,")
        .statement(".thousand_sep .")
        .statement("select 1234567.89::decimal(9,2) as n;")
    )
    result = test.run()
    result.check_stdout("1.234.567,89")

def test_decimal_and_thousand_separator_negative(shell):
    test = (
        ShellTest(shell)
        .statement(".decimal_sep ,")
        .statement(".thousand_sep .")
        .statement("select 1234567.89::decimal(9,2) as p, -123456789.01::decimal(11,2) as n;")
    )
    result = test.run()
    result.check_stdout("1.234.567,89")
    result.check_stdout("-123456789,01")
    result.check_not_exist("-123.456.789,01")

def test_odd_floating_points(shell):
    test = (
        ShellTest(shell)
        .statement("select 1e20, 'nan'::double;")
    )
    result = test.run()
    result.check_stdout("nan")

def test_disable_readable_numbers(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering off")
        .statement("select 123456789;")
    )
    result = test.run()
    result.check_not_exist('(123.46 million)')

def test_large_number_rendering_invalid_bool(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering none")
        .statement("select 123456789;")
    )
    result = test.run()
    result.check_stderr('ERROR: Not a boolean value: "none". Assuming "no".')
    result.check_not_exist('(123.46 million)')

def test_large_number_rendering_all(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering all")
        .statement("select 123456789 from range(10);")
    )
    result = test.run()
    result.check_stdout('123.46 million')
    result.check_not_exist('(123.46 million)')

def test_large_number_rendering_footer_centers_value_row(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering footer")
        .statement("select 1234567 as a, 1234567890123 as b, 999999999999999999 as c;")
    )
    result = test.run()
    result.check_stdout("│    1234567     │  1234567890123  │ 999999999999999999 │")

def test_readable_numbers_columns(shell):
    test = (
        ShellTest(shell)
        .statement(".columns")
        .statement("select 123456789;")
    )
    result = test.run()
    result.check_not_exist('(123.46 million)')

def test_readable_numbers_row_count(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering footer")
        .statement("select r from range(1230000) t(r);")
    )
    result = test.run()
    result.check_stdout('1.23 million rows')

def test_readable_numbers_row_count_wide(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering footer")
        .statement("select r, r, r, r, r, r, r from range(1230000) t(r);")
    )
    result = test.run()
    result.check_stdout('1.23 million rows')

def test_readable_numbers_row_count_wide_single_col(shell):
    test = (
        ShellTest(shell)
        .statement(".large_number_rendering footer")
        .statement("select concat(r, r, r, r, r, r, r) c from range(1230000) t(r);")
    )
    result = test.run()
    result.check_stdout('1.23 million rows')

def test_readable_numbers_footer_streaming_fallback(shell):
    test = (
        ShellTest(shell)
        .statement(".maxrows -1")
        .statement(".large_number_rendering footer")
        .statement("select 123456789;")
    )
    result = test.run()
    result.check_stdout('(123.46 million)')
