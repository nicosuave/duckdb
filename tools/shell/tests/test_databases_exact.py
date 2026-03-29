# fmt: off

from conftest import ShellTest


def test_databases_metadata_render_exact(shell):
    test = (
        ShellTest(shell)
        .statement(".highlight off")
        .statement("ATTACH ':memory:' AS xx")
        .statement(".databases")
    )
    result = test.run()
    expected = """
┌─────────────────┐
│    databases    │
│                 │
│ memory (memory) │
│ xx     (memory) │
└─────────────────┘
""".strip()
    assert result.stdout == expected

