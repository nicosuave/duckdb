# fmt: off

from conftest import ShellTest


def test_duckbox_json_escapes_newlines(shell):
    stmt = """select (
'{\n  "a": [1, 2, 3],\n  "b": {"c": "d"}\n}'
)::json as j"""

    res = (
        ShellTest(shell)
        .statement(".mode duckbox")
        .statement(".headers on")
        .statement(".highlight off")
        .statement(".maxwidth 42")
        .statement(".maxrows 40")
        .statement(stmt)
        .run()
    )

    assert res.stdout == """┌────────────────────────────────────────────┐
│                     j                      │
│                    json                    │
├────────────────────────────────────────────┤
│ {\\n  "a": [1, 2, 3],\\n  "b": {"c": "d"}\\n} │
└────────────────────────────────────────────┘"""
    res.check_stderr(None)


# fmt: on

