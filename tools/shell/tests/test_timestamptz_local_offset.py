from conftest import ShellTest


def test_timestamptz_local_offset(shell):
    res = (
        ShellTest(shell)
        .statement("SET TimeZone='America/Los_Angeles'")
        .statement("select '2026-01-12 12:00:00+00'::timestamptz as ts")
        .run()
    )
    res.check_stdout(
        """┌──────────────────────────┐
│            ts            │
│ timestamp with time zone │
├──────────────────────────┤
│ 2026-01-12 04:00:00-08   │
└──────────────────────────┘"""
    )
    res.check_stderr(None)
