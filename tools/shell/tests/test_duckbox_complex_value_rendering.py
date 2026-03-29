# fmt: off

from conftest import ShellTest


def test_duckbox_complex_value_string_quoting(shell):
    test = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxwidth 120")
        .statement(".headers on")
        .statement(".highlight off")
        .statement(
            "select ['a-b', 'a_b', 'a:b', 'a/b', 'a.b', 'a,b', 'a b', 'true', 'null', '01', '1', '-1', '1.0', 'nan', 'inf'] as xs;"
        )
        .statement(
            "select {'k1': 'a-b', 'k2': 'a b', 'k3': 'a,b', 'k4': 'a\"b', 'k5': 'a''b'} as s;"
        )
        .statement("select [{'a': 1}::json, {'b': [1,2,3]}::json] as js;")
        .statement("select ['{\"a\":1,\"b\":[1,2,3]}', '{\"c\":{\"d\":4}}'] as v;")
    )
    result = test.run()
    expected = """
┌─────────────────────────────────────────────────────────────────────────────────┐
│                                       xs                                        │
│                                    varchar[]                                    │
├─────────────────────────────────────────────────────────────────────────────────┤
│ [a-b, a_b, 'a:b', a/b, a.b, 'a,b', a b, true, 'null', 01, 1, -1, 1.0, nan, inf] │
└─────────────────────────────────────────────────────────────────────────────────┘
┌────────────────────────────────────────────────────────────────────┐
│                                 s                                  │
│ struct(k1 varchar, k2 varchar, k3 varchar, k4 varchar, k5 varchar) │
├────────────────────────────────────────────────────────────────────┤
│ {'k1': a-b, 'k2': a b, 'k3': 'a,b', 'k4': 'a"b', 'k5': 'a\\'b'}     │
└────────────────────────────────────────────────────────────────────┘
┌──────────────────────────┐
│            js            │
│          json[]          │
├──────────────────────────┤
│ [{"a":1}, {"b":[1,2,3]}] │
└──────────────────────────┘
┌──────────────────────────────────────────┐
│                    v                     │
│                varchar[]                 │
├──────────────────────────────────────────┤
│ ['{"a":1,"b":[1,2,3]}', '{"c":{"d":4}}'] │
└──────────────────────────────────────────┘
""".strip()
    assert result.stdout == expected
