import re
from datetime import datetime

from conftest import ShellTest


def local_offset_for_shell_display() -> str:
    dt = datetime.now().astimezone()
    offset = dt.utcoffset()
    if offset is None:
        return "+00"
    total_minutes = int(offset.total_seconds() // 60)
    sign = "+" if total_minutes >= 0 else "-"
    total_minutes = abs(total_minutes)
    hours, minutes = divmod(total_minutes, 60)
    if minutes == 0:
        return f"{sign}{hours:02d}"
    return f"{sign}{hours:02d}:{minutes:02d}"


def extract_timestamptz_offset_from_box(stdout: str) -> str:
    for line in stdout.splitlines():
        if "│" not in line:
            continue
        if not re.search(r"\d{4}-\d{2}-\d{2}", line):
            continue
        m = re.search(r"([+-]\d{2}(?::\d{2})?)\s*│\s*$", line)
        if m:
            return m.group(1)
    raise AssertionError(f"could not find timestamptz row in output:\n{stdout}")


def test_timestamptz_default_uses_local_timezone_not_env_tz(shell):
    expected = local_offset_for_shell_display()

    # Match shipped shell behavior: the session defaults to local timezone regardless of TZ env var.
    binary = ShellTest(shell).env_var("TZ", "UTC")
    binary.statement("select now()")
    res = binary.run()
    assert res.status_code == 0

    offset = extract_timestamptz_offset_from_box(res.stdout)
    assert offset == expected

