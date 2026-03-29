# fmt: off

from conftest import ShellTest


def test_duckbox_pretty_print_nested_matches_shell(shell):
    result = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".maxwidth 30")
        .statement(".maxrows 60")
        .statement(".headers on")
        .statement(".highlight off")
        .statement("select [{'a': [1,2,3], 'b': {'c': [4,5], 'd': 6}}, {'e': [7,8,9], 'f': {'g': [10,11], 'h': 12}}] as v")
        .statement("select {'a': [1,2,3], 'b': [{'x': [1,2,3]}, {'y': [4,5]}], 'c': {'d': {'e': 1}}} as s")
        .run()
    )
    expected = """
┌──────────────────────────────────────────────────────────────────────────────┐
│                                      v                                       │
│ struct(a integer[], b struct(c integer[], d integer), e integer[], f struct( │
│                          g integer[], h integer))[]                          │
├──────────────────────────────────────────────────────────────────────────────┤
│ [                                                                            │
│  {'a': [1, 2, 3], 'b': {'c': [4, 5], 'd': 6}, 'e': null, 'f': null},         │
│  {'a': null, 'b': null, 'e': [7, 8, 9], 'f': {'g': [10, 11], 'h': 12}}       │
│ ]                                                                            │
└──────────────────────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────┐
│                                      s                                       │
│ struct(a integer[], b struct(x integer[], y integer[])[], c struct(d struct( │
│                                 e integer)))                                 │
├──────────────────────────────────────────────────────────────────────────────┤
│ {                                                                            │
│   'a': [1, 2, 3],                                                            │
│   'b': [{'x': [1, 2, 3],                                                     │
│      'y': null                                                               │
│    },                                                                        │
│    {'x': null, 'y': [4, 5]}                                                  │
│   ],                                                                         │
│   'c': {'d': {'e': 1}}                                                       │
│ }                                                                            │
└──────────────────────────────────────────────────────────────────────────────┘
""".strip()
    assert result.stdout == expected


# fmt: on
