# Rust CLI DuckDB 1.5.x Upgrade Goal

Date: 2026-05-24

Branch: `rust-cli`

Target DuckDB release: `v1.5.3`

Status: complete for the larger DuckDB Rust CLI 1.5.x parity changeset on macOS and Linux arm64 packaging.

## Objective

Implement the larger DuckDB Rust CLI 1.5.x changeset, not just document it:

- Retarget the Rust CLI from DuckDB `1.4.3` to DuckDB `1.5.3`.
- Close the high-risk CLI parity gaps found during the 1.5.x assessment.
- Verify the full macOS shell behavior suite and parity diff scripts against the official `v1.5.3` shell.
- Verify Linux packaging in a Linux arm64 Docker environment.
- Commit the completed changeset only after verification.

## Completed Changeset

| Area | Result | Verification |
|---|---|---|
| Runtime and packaging target | Default vendor/runtime/package target is DuckDB `1.5.3`. | `./target/debug/duckdb_cli -version`, macOS and Linux package smoke tests |
| Linux portability | Fixed `duckdb_open_ext` error pointer typing for Linux arm64 bindings. | Linux arm64 Docker package build |
| Prompt rendering | Added dynamic prompt parser/rendering for `{setting}`, `{sql}`, `{color}`, `{highlight_element}`, `{max_length}`, and default `memory D` prompt behavior. | `tools/shell/tests/test_prompt.py`, full shell suite |
| Prompt validation | `.prompt` validates only the main prompt and leaves continuation prompts literal, matching official behavior. | `tools/shell/tests/test_prompt.py` |
| Progress bar templates | Added validation for progress-bar template components and alignment wrappers. | `tools/shell/tests/test_prompt.py` |
| Help metadata | Added `.help shortcuts` and aligned `.help` text/footer details for 1.5.x command metadata. | `tools/shell/tests/test_help_visibility.py`, `test_help_no_ansi.py` |
| Storage version CLI | Aligned invalid `-storage-version` handling with the 1.5.3 CLI error shape. | `tools/shell/tests/test_command_line_arguments.py` |
| Pager behavior | Aligned `.pager` status text, rejected unsupported `set_column_threshold`, and disabled pager for non-interactive stdin. | `tools/shell/tests/test_pager.py` |
| Colors and highlighting | Matched `.display_colors` tie ordering and retained highlight-mode behavior. | `tools/shell/tests/test_display_colors.py`, highlighting tests |
| Warnings/logging | Added duplicate warning suppression and log-level ANSI styling. | `tools/shell/tests/test_warning.py`, `test_logging.py` |
| Interactive readline | Updated vendored 1.5.3 linenoise ABI and Rust FFI for completion type/score/extra char plus seeded reverse search. | `tools/shell/tests/test_autocomplete.py`, `test_interactive_startup.py`, PTY tests |
| Vendored linenoise | Replaced the 1.5.3 linenoise vendor tree and added minimal shell shims needed for Rust integration. | macOS build, Linux build, full shell suite |
| Tests | Added focused regression coverage for every parity gap fixed in this pass. | Full shell suite and targeted shell slice |

## Upgrade Inventory

| Area | Final status | Notes |
|---|---|---|
| DuckDB runtime/version | Done | Builds and reports `v1.5.3`. |
| C API bindings | Done for used surface | Linux arm64 caught and verified the `c_char` portability fix. |
| macOS packaging | Done | Produces `rust_cli/dist/duckdb-rust-cli-1.5.3-macos-arm64.tar.gz`. |
| Linux packaging | Done for arm64 | Produces `/tmp/duckdb-rust-cli-linux-package-out/duckdb-rust-cli-1.5.3-linux-aarch64.tar.gz` in Docker and smoke-runs it. |
| CLI flags/init/bail/safe mode | Previously implemented and reverified | Covered by the full 1.5.3 shell suite. |
| `.open`, `.dump`, `.import`, `.last`, `_` | Previously implemented and reverified | Covered by the full 1.5.3 shell suite. |
| Rendering modes/DuckBox/JSON | Previously implemented and reverified | Covered by the full shell suite and parity diff scripts. |
| Table metadata rendering | Previously implemented and reverified | Covered by the full 1.5.3 shell suite. |
| Pager | Done in this pass | Status, rejection, and non-interactive behavior aligned. |
| Prompt | Done in this pass | Dynamic prompt components now render instead of only validating. |
| Progress bar | Done for 1.5.x validation surface | Component validation added; full live progress UI remains limited by testability. |
| Highlighting/colors | Done in this pass | Added display-color ordering regression. |
| Warnings/logging | Done in this pass | Added duplicate-warning and logging coverage. |
| Interactive editing | Done in this pass | Completion ABI and reverse search parity covered by PTY tests. |
| Width/Unicode | Previously implemented and reverified | Covered by full shell suite and parity scripts. |
| Windows | Out of scope | This goal covers macOS and Linux packaging. |

## Verification Record

Recorded on 2026-05-24 from branch `rust-cli`.

| Check | Result |
|---|---|
| `CARGO_TARGET_DIR=/Users/nico/Code/duckdb-rust-cli/target DUCKDB_VENDOR_VERSION=1.5.3 DUCKDB_LIB_SOURCE=repo cargo test -p duckdb_cli` | Pass, 0 Rust unit tests |
| `bash rust_cli/run_shell_tests.sh tools/shell/tests/test_prompt.py tools/shell/tests/test_help_visibility.py tools/shell/tests/test_help_no_ansi.py tools/shell/tests/test_command_line_arguments.py tools/shell/tests/test_display_colors.py tools/shell/tests/test_highlight_colors_extended.py tools/shell/tests/test_deprecated_highlight_color_aliases.py tools/shell/tests/test_highlight_mode_invalid.py tools/shell/tests/test_highlighting.py tools/shell/tests/test_logging.py tools/shell/tests/test_warning.py tools/shell/tests/test_pager.py tools/shell/tests/test_autocomplete.py tools/shell/tests/test_interactive_startup.py tools/shell/tests/test_interactive_ctrl_a_ctrl_e.py tools/shell/tests/test_interactive_ctrl_c.py tools/shell/tests/test_interactive_ctrl_d.py tools/shell/tests/test_interactive_ctrl_u_ctrl_k.py tools/shell/tests/test_interactive_ctrl_w.py tools/shell/tests/test_interactive_history_navigation.py tools/shell/tests/test_interactive_statement_splitting.py tools/shell/tests/test_sql_is_complete.py tools/shell/tests/test_statement_splitting_edgecases.py tools/shell/tests/test_read_from_stdin.py` | Pass, `132 passed` |
| `bash rust_cli/run_shell_tests.sh` | Pass, `495 passed, 2 skipped` |
| `DUCKDB_VENDOR_VERSION=1.5.3 bash rust_cli/package.sh` | Pass, produced `rust_cli/dist/duckdb-rust-cli-1.5.3-macos-arm64.tar.gz` |
| Linux arm64 Docker package build with `rust:1.90-bookworm` and DuckDB `1.5.3` Linux arm64 library | Pass, produced `/tmp/duckdb-rust-cli-linux-package-out/duckdb-rust-cli-1.5.3-linux-aarch64.tar.gz` |
| Linux arm64 package smoke: `duckdb -version` | Pass, printed `v1.5.3` |
| Linux arm64 package smoke: `duckdb -c "select 42 as answer;"` | Pass, returned `42` |
| `DUCKDB_REF_SHELL=/opt/homebrew/bin/duckdb DUCKDB_RUST_SHELL=./target/debug/duckdb_cli bash rust_cli/diff_shells.sh rust_cli/parity_smoke.sql` | Pass |
| `DUCKDB_REF_SHELL=/opt/homebrew/bin/duckdb DUCKDB_RUST_SHELL=./target/debug/duckdb_cli bash rust_cli/diff_shells.sh rust_cli/parity_modes_more.sql` | Pass |
| `DUCKDB_REF_SHELL=/opt/homebrew/bin/duckdb DUCKDB_RUST_SHELL=./target/debug/duckdb_cli bash rust_cli/diff_shells.sh rust_cli/parity_render_quirks.sql` | Pass |
| `DUCKDB_REF_SHELL=/opt/homebrew/bin/duckdb DUCKDB_RUST_SHELL=./target/debug/duckdb_cli bash rust_cli/diff_shells.sh rust_cli/parity_duckbox_json.sql` | Pass |
| `DUCKDB_REF_SHELL=/opt/homebrew/bin/duckdb DUCKDB_RUST_SHELL=./target/debug/duckdb_cli bash rust_cli/diff_shells.sh rust_cli/parity_duckbox_json_more.sql` | Pass |

## Definition Of Done

| Requirement | Status |
|---|---|
| Rust CLI builds and runs against DuckDB `1.5.3`. | Done |
| Package names and runtime version report `1.5.3`. | Done |
| P0/P1/P2 parity gaps found by the 1.5.x assessment are implemented or explicitly scoped. | Done |
| Full shell suite passes after the final code change. | Done |
| Targeted parity/oracle checks pass after the final code change. | Done |
| macOS package is produced successfully. | Done |
| Linux package path is verified on Linux or CI. | Done on Linux arm64 Docker |
| Completed changeset is committed after verification. | Done by the commit containing this file |

## Follow-Up Notes

- The current build still emits existing Rust warnings for unused helpers and two `total_render_length` assignments in `exec.rs`.
- Windows packaging remains out of scope until explicitly requested.
- The Rust REPL keeps the vendored linenoise editor interception disabled to avoid C++ exceptions unwinding over Rust FFI.
- Future CLI parity work should continue using the official `v1.5.3` shell tests and `rust_cli/diff_shells.sh` scripts as the oracle.
