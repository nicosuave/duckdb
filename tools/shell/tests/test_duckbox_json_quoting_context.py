# fmt: off

from conftest import ShellTest


def test_duckbox_json_quoting_context(shell):
    test = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxwidth 120")
        .statement(".headers on")
        .statement(".highlight off")
        .statement("select map(['a','b'], [json('{\"a\":1}'), json('{\"b\":2}')]) as jmap;")
        .statement(
            "select {'a': json('{\"a\":1}'), 'b': [json('{\"b\":2}'), json('{\"c\":3}')]} as jstruct;"
        )
        .statement("select [json('{\"a\":1}'), json('{\"b\":2}')] as jlist;")
        .statement("select {'a': [ {'x': [1,2,3]}::json, {'y': [4,5]}::json ], 'b': 1} as s;")
    )
    result = test.run()
    expected = """
┌────────────────────────────┐
│            jmap            │
│     map(varchar, json)     │
├────────────────────────────┤
│ {a='{\"a\":1}', b='{\"b\":2}'} │
└────────────────────────────┘
┌───────────────────────────────────────────┐
│                  jstruct                  │
│         struct(a json, b json[])          │
├───────────────────────────────────────────┤
│ {'a': '{\"a\":1}', 'b': [{\"b\":2}, {\"c\":3}]} │
└───────────────────────────────────────────┘
┌────────────────────┐
│       jlist        │
│       json[]       │
├────────────────────┤
│ [{\"a\":1}, {\"b\":2}] │
└────────────────────┘
┌─────────────────────────────────────────────┐
│                      s                      │
│         struct(a json[], b integer)         │
├─────────────────────────────────────────────┤
│ {'a': [{\"x\":[1,2,3]}, {\"y\":[4,5]}], 'b': 1} │
└─────────────────────────────────────────────┘
""".strip()
    assert result.stdout == expected
