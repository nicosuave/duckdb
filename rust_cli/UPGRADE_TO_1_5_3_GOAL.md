# Rust CLI DuckDB 1.5.3 Upgrade Goal

Date: 2026-05-24

Branch: `rust-cli`

Baseline commit: `abcd5d8929 Add Rust CLI for DuckDB`

Target DuckDB release: `v1.5.3`

## Objective

Upgrade the Rust DuckDB CLI from its current DuckDB `1.4.3` target to DuckDB `1.5.3`, preserving drop-in CLI behavior against the official shell test suite and the Rust-specific parity tests already added on the branch.

The upgrade is complete when the Rust CLI builds, packages, and passes the relevant `v1.5.3` shell behavior tests with matching stdout, stderr, and exit-code behavior on macOS and Linux.

## Context

The Rust CLI branch is already a full Rust implementation of the newer DuckDB shell UX shape. It is not a rewrite of the old SQLite-wrapper shell. The branch was created after much of the upstream C++ shell refactor had landed, but the Rust implementation still pins and packages DuckDB `1.4.3`.

The official DuckDB CLI changed substantially across `v1.4.3 -> v1.5.3`:

| Scope | Change size |
|---|---:|
| `tools/shell` plus `tools/sqlite3_api_wrapper` | 72 files, 12197 insertions, 22758 deletions |
| `tools/shell/tests` | 21 files, 3206 insertions, 231 deletions |
| Shell test files | 18 to 31 |
| Approximate shell test definitions | 197 to 300 |

The highest-risk surface is behavior parity, not just linking a newer `libduckdb`.

## Upgrade Inventory

| Area | Current Rust CLI status | Required upgrade | Priority | Verification |
|---|---|---|---:|---|
| DuckDB runtime/version | Defaults to `1.4.3` in `crates/duckdb_cli/build.rs`, `crates/duckdb_sys/build.rs`, and `rust_cli/package.sh` | Bump default vendor target to `1.5.3`; verify vendor layout, headers, library naming, and runtime version check | P0 | Build and runtime version smoke |
| C API bindings | Uses committed/generated bindings for the current target | Regenerate or update `duckdb_sys` bindings for `1.5.3`; verify optional dynamic symbols still resolve | P0 | `cargo build -p duckdb_cli` |
| Packaging | Produces `duckdb-rust-cli-1.4.3-*` artifacts | Produce `duckdb-rust-cli-1.5.3-*` artifacts for macOS and Linux | P0 | `DUCKDB_VENDOR_VERSION=1.5.3 bash rust_cli/package.sh` |
| CLI flags | Missing `-h`, command-line `-jsonlines`, and `--no-init` | Add flags to `crates/duckdb_cli/src/options.rs`; implement init skipping | P0 | `tools/shell/tests/test_command_line_arguments.py`, `test_shell_basics.py` |
| Init and rc handling | Has `.duckdbrc` and `-init`, no skip flag | Match `run_init=false` behavior and init failure handling in `v1.5.3` | P0 | Init and command-line tests |
| Bail behavior | Boolean `bail_on_error` | Replace with tri-state `auto`, `on`, `off`; update `.bail`, `-bail`, `-cmd`, `-c`, `-s`, `-f`, `.read`, and init handling | P0 | Bail tests in shell suite |
| `.help shortcuts` | Not implemented | Add shortcut help for Rust line editor, or explicitly map to current linenoise shortcut table | P1 | `.help shortcuts` transcript |
| `-storage-version` | Present as config string | Verify `1.5.3` accepted values and error behavior match official shell | P1 | Command-line argument tests |
| Dot-command metadata | Broad command table already exists | Reconcile command ordering, match sizes, help text, aliases, and extended help with `v1.5.3` | P1 | `.help`, `.help --all`, completion tests |
| `.open` | Implements `--sql`, `--new`, `--nofollow`, `--readonly` | Verify current `v1.5.3` errors and file-relation behavior for DuckDB, Parquet, CSV, SQL expressions | P0 | `tools/shell/tests/test_open.py` |
| `.dump` | Implemented with known parity fixes | Verify schema-qualified tables, quoted schemas/tables, views, indexes, blobs, newlines, ordering | P0 | `tools/shell/tests/test_dump.py` |
| `.import` | Implemented | Verify CSV, JSON, Parquet and generic reader parameter behavior | P1 | `tools/shell/tests/test_import.py` |
| Last result `_` | Implemented through temp table and replacement scan | Verify no-result error, errors not clobbering `_`, chained `_`, and temp table lifetime | P0 | `tools/shell/tests/test_last_result.py` |
| `.last` | Implemented for DuckBox untruncation | Verify full-result replay and truncation hints | P1 | `test_last_result.py`, Rust parity tests |
| Rendering modes | All main modes implemented | Run and patch against full `v1.5.3` rendering oracle | P0 | `tools/shell/tests/test_rendering_mode_regression.py` |
| JSON and JSON Lines | Existing implementation has typed JSON coverage | Verify nested list, struct, map, JSON logical type, booleans, decimals, infinity, NaN, and empty results | P0 | JSON/jsonlines tests |
| DuckBox | Large manual port | Reconcile `max_rows`, `max_width`, `max_analyze_rows`, hidden rows/columns, footer hints, wrapping, truncation, large numbers | P0 | `test_large_value_rendering.py`, `test_shell_rendering.py` |
| Table metadata rendering | Implemented manually | Verify `.tables`, `.databases`, `show tables`, `show schemas`, `DESCRIBE`, attached DBs, search path, long names | P0 | `test_table_metadata_rendering.py`, `test_schema_metadata_rendering.py` |
| Pager | Implemented, with extra column threshold support | Match official `automatic/on/off`, env precedence, batch disable, output-file disable, no pager for dump formats | P1 | `tools/shell/tests/test_pager.py` |
| Prompt | Parser validation exists, dynamic rendering is likely partial | Implement or verify `{setting}`, `{sql}`, `{color}`, `{highlight_element}`, `{max_length}`, dynamic DB/schema prompt | P1 | `tools/shell/tests/test_prompt.py` |
| Progress bar | State and components exist, parity not fully validated | Match component renderer for percent, ETA, bytes read/written, memory, swap, alignment, min size, hide-if-contains | P2 | Manual long-query smoke plus focused tests |
| Highlighting/colors | Partial 256-color and mode support | Align `.display_colors`, `.highlight_mode`, result/error/table/log color element behavior | P1 | `test_highlighting.py`, Rust color tests |
| Warnings/logging | `shell_log_storage` exists | Verify warning dedupe, stdout/stderr routing, `warnings_as_errors`, log-level formatting, colors | P1 | `tools/shell/tests/test_warning.py` |
| Safe mode | Implemented | Verify config locking and restricted commands under `-safe` | P0 | `tools/shell/tests/test_safe_mode.py` |
| Interactive editing | Rust linenoise shim exists | Verify completion enter handling, reverse search seeded from buffer, Ctrl-C shutdown, multiline/singleline argument checks | P2 | Autocomplete and PTY tests |
| Width/Unicode | Manual render-width code exists | Verify grapheme clusters, ANSI escapes, tabs, newlines, underflow and divide-by-zero cases | P1 | Large-value, Unicode, and rendering tests |
| Windows | Existing Rust goal is macOS/Linux only | Keep out of scope unless explicitly added | P3 | None for this goal |

## Execution Plan

1. Establish a clean `v1.5.3` test baseline.
   - Confirm latest target tag is available locally.
   - Build official shell at `v1.5.3` if needed for output comparison.
   - Record official shell behavior for high-risk focused tests.

2. Retarget runtime and bindings.
   - Change default vendor version from `1.4.3` to `1.5.3`.
   - Update vendored headers and library lookup paths if needed.
   - Regenerate or patch C bindings.
   - Build Rust CLI against `1.5.3`.

3. Fix P0 command semantics.
   - Add `-h`, `-jsonlines`, `--no-init`.
   - Replace boolean bail state with tri-state bail.
   - Align init, rc, file, read, and command bail behavior.
   - Verify safe mode config locking.

4. Fix P0 behavior parity.
   - Rendering modes, especially JSON/JSON Lines and DuckBox.
   - Table metadata rendering.
   - `.open`, `.dump`, `.last`, and `_`.

5. Fix P1 parity.
   - Pager, prompt, highlighting, warnings/logging, storage-version, help metadata.

6. Decide on P2/P3 scope.
   - Progress bar and interactive edge cases should be fixed if they fail current tests.
   - Windows remains out of scope unless requested.

## Verification Steps

Run these from the Rust CLI worktree.

```bash
git status --short
git branch --show-current
```

Expected:

- Branch is `rust-cli`.
- No unrelated files are modified.

Build checks:

```bash
DUCKDB_VENDOR_VERSION=1.5.3 DUCKDB_LIB_SOURCE=repo cargo build -p duckdb_cli
cargo test -p duckdb_cli
```

Packaging check:

```bash
DUCKDB_VENDOR_VERSION=1.5.3 bash rust_cli/package.sh
```

Full shell-suite check:

```bash
bash rust_cli/run_shell_tests.sh
```

Focused high-risk checks:

```bash
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_command_line_arguments.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_shell_basics.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_rendering_mode_regression.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_large_value_rendering.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_shell_rendering.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_table_metadata_rendering.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_schema_metadata_rendering.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_open.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_dump.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_import.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_last_result.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_pager.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_warning.py
bash rust_cli/run_shell_tests.sh tools/shell/tests/test_safe_mode.py
```

Behavior comparison checks:

```bash
bash rust_cli/diff_shells.sh rust_cli/parity_smoke.sql
bash rust_cli/diff_shells.sh rust_cli/parity_modes_more.sql
bash rust_cli/diff_shells.sh rust_cli/parity_render_quirks.sql
bash rust_cli/diff_shells.sh rust_cli/parity_duckbox_json.sql
bash rust_cli/diff_shells.sh rust_cli/parity_duckbox_json_more.sql
```

Manual smoke checks:

```bash
./target/debug/duckdb_cli -h
./target/debug/duckdb_cli -jsonlines -c "select true as b, [1,2] as xs, {'k': 3} as s"
./target/debug/duckdb_cli --no-init -c "select 42"
./target/debug/duckdb_cli -safe -c "set memory_limit='-1'"
```

## Definition Of Done

- `duckdb_cli` builds against DuckDB `1.5.3`.
- `duckdb_sys` bindings match the `1.5.3` public C API used by the Rust CLI.
- Runtime version checks and package names report `1.5.3`.
- All P0 rows in the upgrade inventory are implemented and verified.
- The full shell test suite passes through `bash rust_cli/run_shell_tests.sh`, or any remaining skips/failures are documented with explicit rationale.
- Existing Rust-specific parity tests still pass.
- macOS package is produced successfully.
- Linux package path is verified or documented with the exact remaining blocker.
- The final commit message mentions the `1.5.3` upgrade scope and test result summary.

## Notes For Future Agents

- Do not treat this as only a `libduckdb` bump. The `1.5.x` CLI added a large behavior surface.
- Use the official `v1.5.3` shell tests as the oracle.
- Keep the original `rust_cli/PLAN.md` as the historical `1.4.3` implementation plan.
- Add targeted regression tests when a parity mismatch is found.
- Keep Windows out of scope unless the user explicitly expands the goal.
