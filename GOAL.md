# DuckDB Rust CLI 1.5.3 Parity Goal

Date: 2026-05-25

Branch: `rust-cli`

Baseline commit: `3b91253cc5 Complete Rust CLI 1.5.x parity upgrades`

Target reference CLI: DuckDB `v1.5.3`

Goal status: complete

## Objective

Close the remaining known deltas between the Rust DuckDB CLI and the official DuckDB `1.5.3` CLI, then verify the completed result against the official CLI test suite, parity transcript scripts, and platform package smoke tests.

This goal is not to retarget DuckDB again. The current Rust CLI already runs against DuckDB `v1.5.3`. This goal is specifically about the remaining parity shortcomings and verification holes found after the 1.5.3 upgrade.

## Current Baseline

| Check | Baseline result |
|---|---|
| Runtime target | Rust CLI reports DuckDB `v1.5.3` |
| Commit | `3b91253cc5 Complete Rust CLI 1.5.x parity upgrades` |
| Rust unit tests | `10 passed` |
| Full shell test harness | `509 passed, 2 skipped` after the final fix pass |
| Targeted 1.5.x shell slice | `132 passed` |
| Parity transcript scripts | 5 passed against `/opt/homebrew/bin/duckdb` `v1.5.3` |
| macOS arm64 package | Built and smoke-tested |
| Linux arm64 package | Built and smoke-tested in Docker |
| Linux x86_64 package | Built and smoke-tested in Docker |
| Known scope | macOS and Linux CLI parity; Windows and signing/notarization are out of scope |

## Functionality Inventory

| Area | Current status | Confidence | Remaining work |
|---|---|---:|---|
| Runtime/library target | Runs against DuckDB `v1.5.3` | High | Keep version checks after any build/package changes |
| Command-line options | Visible 1.5.3 option list is implemented | High | Version formatting fixed; keep help/version checks in package smoke |
| Dot-command surface | Visible 1.5.3 command list is implemented | High | Fix behavioral deltas below |
| Rendering modes | Main modes implemented and heavily tested | High | Add focused tests for any new rendering fixes |
| DuckBox rendering | High parity for tested surface | Medium-high | Cover rare extension/logical types if touched |
| JSON/JSONLines | High parity for tested surface | High | Add edge tests for rare nested/extension values if found |
| `.dump` | Mostly implemented | Medium-high | `--preserve-rowids` rejection fixed; continue rare object coverage |
| `.import` | Implemented for CSV/JSON/Parquet/generic params | Medium-high | Dotted-table and generic-parameter coverage added; keep extension-backed formats passing |
| `.open` | Implemented for 1.5.3 flags | Medium-high | `--nofollow` symlink behavior now covered; keep `--sql` matrix passing |
| Last result `_` and `.last` | Implemented and tested | Medium-high | Add multi-statement edge tests if behavior changes |
| Metadata commands | Implemented and tested for common cases | Medium-high | Add exotic catalog object coverage if needed |
| Safe mode | Implemented and tested | High | Keep full safe-mode suite passing |
| Prompt | Dynamic 1.5.3 prompt implemented | High | Verify any new runtime metric support |
| Progress bar | Component validation implemented; default C API renderer leakage fixed | Medium | Reference 1.5.3 PTY does not print live progress for the tested path; future custom renderer would be a separate enhancement |
| Pager | Implemented and tested for command/status behavior and DuckBox automatic PTY paging | High | Keep fake-pager PTY regression passing |
| Autocomplete/readline | Strong parity through vendored 1.5.3 linenoise | Medium-high | Keep `.multiline`/`.singleline`, editor alias, reverse-search, and autocomplete PTY regressions passing |
| Highlighting/colors | Implemented and tested for exposed commands plus interactive SQL ANSI smoke | Medium-high | Exact color/style transcript remains terminal-dependent |
| Warnings/logging | Implemented and tested for current API symbols | Medium-high | HTTP file logging skip is documented as removed/deprecated official behavior |
| UI command | `.ui_command` and `-ui` launch wiring covered with controlled failing command | Medium-high | Real UI extension launch remains outside CLI parity |
| Packaging | macOS arm64, Linux arm64, and Linux x86_64 verified | High | Signing/notarization remains a separate distribution-hardening goal |
| Rust unit tests | Focused parser/renderer unit tests added | Medium | Expand if future parser/rendering changes add new risk |

## Known Behavior Deltas

| ID | Shortcoming | Current Rust behavior | Official 1.5.3 behavior | Priority | Required outcome | Verification |
|---|---|---|---|---:|---|---|
| B1 | `.edit` parity needed confirmation | Interactive `.edit` opens external editor; batch `.edit` is unsupported | Official interactive `.edit` opens editor; batch `.edit` is unsupported | P1 confirmed | Keep official-compatible split between interactive and batch behavior | Existing `.edit` PTY test passes; batch manual probe matches official |
| B2 | `\e` editor alias crashed through linenoise continuation/render path | `\e` was treated as incomplete SQL before Rust could handle the alias | Official supports `\e` as `.edit` alias | P1 fixed | Return standalone `\e` to Rust and use the existing Rust-side editor path | New interactive PTY test for `\e` passes |
| B3 | `.multiline` and `.singleline` were accepted no-ops | Commands returned success but did not change linenoise physical line editing mode | Official toggles linenoise physical line editing mode; this does not change SQL semicolon completeness | P1 fixed | Wire these commands to `linenoiseSetMultiLine` | Code path wired; official behavior checked to avoid incorrect SQL-submission tests |
| B4 | Rust-only `.dump --preserve-rowids` | Rust accepted it | Official 1.5.3 rejects it | P1 fixed | Reject it with official-compatible error unless a deliberate Rust-only extension is approved | New batch `.dump --preserve-rowids` test passes |
| B5 | Version output detail mismatch | `-version`/`.version` emitted shorter output | Official includes codename/source id and `.version` compiler line | P2 fixed | Match official text where possible | New `-version` and `.version` tests pass against Rust and `/opt/homebrew/bin/duckdb` |
| B6 | Live progress bar parity not proven | Rust printed DuckDB's default C API progress renderer, ignoring `.progress_bar` components | Reference DuckDB 1.5.3 PTY did not print a live progress bar for the same long-running query | P1 fixed | Disable default print renderer so Rust does not emit non-reference progress lines; keep component validation | New PTY regression passes against Rust and `/opt/homebrew/bin/duckdb` |
| B7 | DuckBox automatic pager behavior not fully proven | Rust only paged DuckBox output when `.pager on` was set | Official pages interactive DuckBox output in automatic mode when thresholds fire | P2 fixed | Use the same automatic row/column threshold decision for DuckBox writer startup | New PTY fake-pager test passes against Rust and `/opt/homebrew/bin/duckdb` |
| B8 | Editing-side highlight element parity uses minimal shim | Rust result/error highlight controls pass tests; vendored linenoise uses a minimal shell highlight shim | Official emits interactive SQL ANSI highlighting when enabled | P2 covered | Keep vendored linenoise highlighter active and catch tokenizer exceptions across FFI; exact style mapping is terminal-dependent | New PTY ANSI-highlighting smoke passes against Rust and `/opt/homebrew/bin/duckdb` |
| B9 | UI launch path not deeply validated | `-ui` queues `CALL start_ui()` and `.ui_command` sets command | Official launches the configured UI command | P2 covered | Test `.ui_command` plus `-ui` using a controlled missing function instead of requiring the real UI extension | New `-ui`/`.ui_command` test passes against Rust and `/opt/homebrew/bin/duckdb` |
| B10 | Log storage degrades silently if symbols are unavailable | Rust uses best-effort `dlsym` for 1.5.x logging hooks | Official shell has native log storage integration | P2 covered | Keep best-effort dynamic symbol behavior and rely on shell-log-storage tests to fail if symbols/register path are unavailable | `test_warning.py` and `test_logging.py` pass |

## Known Verification Gaps

| ID | Gap | Why it matters | Priority | Required outcome | Verification |
|---|---|---|---:|---|---|
| V1 | Two shell tests are skipped | Current pass is `509 passed, 2 skipped`, not zero skips | P1 documented | Both skips are stale/non-applicable for official 1.5.3 parity | Full suite skip report captured; skip reasons updated |
| V2 | `.eqp full` / `db_config` skip | Official 1.5.3 rejects `.eqp`; Rust matches that behavior | P1 documented out of scope | Keep skipped as legacy SQLite-shell coverage, not DuckDB 1.5.3 parity | Manual official/Rust probe and updated skip reason |
| V3 | HTTP file logging skip | Official 1.5.3 deprecates `http_logging_output`; test targets removed behavior | P1 documented out of scope | Keep skipped unless a future goal revives deprecated HTTP logging behavior | Manual official probe and updated skip reason |
| V4 | `diff_shells.sh` ignored command exit status before diffing output | Transcript parity could pass while exit codes differed | P1 fixed | Capture and compare exit codes in parity scripts | Exit-code comparison added; all five parity SQL scripts pass |
| V5 | No Rust unit tests | Complex parser/rendering logic is tested mostly through shell tests | P2 fixed | Added focused unit tests for prompt parsing, progress templates, display color ordering, and JSON formatter | `cargo test -p duckdb_cli` passes with 10 tests |
| V6 | `.open --nofollow` lacked focused symlink coverage | Source parses flag, but behavior was not specifically proven | P2 fixed | Cover reference-compatible symlink behavior | New symlink test passes against Rust and `/opt/homebrew/bin/duckdb` |
| V7 | `.import` dotted-table and parameter matrix was not exhaustive | Rust treated dotted import targets as schema-qualified while official treats them as literal table names | P2 fixed | Match official dotted-table behavior and cover representative generic params | New `test_import.py` cases pass against Rust and `/opt/homebrew/bin/duckdb` |
| V8 | `.output`/`.once` BOM/editor/spreadsheet paths not deeply verified | External file/app behavior can diverge | P2 fixed | Added deterministic file/BOM/reset tests; external app launch remains out of scope for noninteractive CI | New `.once` reset, `.once --bom`, and `.output stdout` tests pass against Rust and `/opt/homebrew/bin/duckdb` |
| V9 | Rare catalog objects not fully covered in `.dump`/metadata | Macros/sequences/views/index edge cases can differ | P2 documented | Added constraint/view/index coverage and documented official 1.5.3 omission of sequence/macro DDL from `.dump` | New dump catalog test passes against Rust and `/opt/homebrew/bin/duckdb` |
| V10 | Extension logical types are workaround-based | VARIANT/geometry/future types may format differently | P2 covered | Use existing VARIANT, JSON, and all-types geometry rendering tests as focused coverage | Variant/nested JSON/logical-type shell tests pass |
| V11 | Linux x86_64 artifact not verified | Linux evidence was arm64 Docker only | P2 fixed | Build/smoke Linux x86_64 package | Docker linux/amd64 package build and `select 42` smoke pass |
| V12 | Windows behavior out of scope | Official CLI has Windows-specific `.utf8` and skip conditions | P3 documented out of scope | Keep Windows out of this macOS/Linux CLI parity goal | Written scope decision; no accidental Windows claims |
| V13 | macOS signing/notarization not checked | Packaging is functional but not distribution-hardened | P3 documented out of scope | Treat signing/notarization as a separate distribution-hardening goal | Documented non-goal; package smoke proves functional tarball |
| V14 | Extension-dependent tests can skip dynamically | Autocomplete/HTTP/json availability can alter coverage | P2 documented | Keep skip reasons explicit and record extension-sensitive focused tests in the progress log | Full-suite skip report and focused extension/logical-type tests |

## Implementation Plan

| Step | Work | Expected output |
|---|---|---|
| 1 | Re-run inventory baseline with skip reasons and direct official/Rust probes for known deltas | Updated checklist with current failures/repros |
| 2 | Fix P1 behavior deltas: `.edit`/`\e`, `.multiline`/`.singleline`, `.dump --preserve-rowids`, progress-bar decision, skipped tests | Code changes plus shell/PTY tests |
| 3 | Fix P1 verification gaps: exit-code parity in `diff_shells.sh`, full suite skip decision | Improved parity harness and documentation |
| 4 | Work P2 coverage gaps by risk: pager live DuckBox, `.open --nofollow`, `.import` matrices, UI command, highlight audit, logging symbols | Focused tests and fixes |
| 5 | Add targeted Rust unit tests for pure parsers/renderers | Non-zero useful `cargo test -p duckdb_cli` coverage |
| 6 | Re-run full verification matrix | Passing test/package/parity record |
| 7 | Update this `GOAL.md` with final status and commit the completed changeset only after verification | Final committed goal record |

## Verification Matrix

Run from `/Users/nico/Code/duckdb-rust-cli`.

| Check | Command |
|---|---|
| Working tree/branch | `git status --short` and `git branch --show-current` |
| Rust tests | `CARGO_TARGET_DIR=$PWD/target DUCKDB_VENDOR_VERSION=1.5.3 DUCKDB_LIB_SOURCE=repo cargo test -p duckdb_cli` |
| Full shell suite | `bash rust_cli/run_shell_tests.sh -rs` |
| Targeted known-delta shell tests | Run individual new tests for `.edit`, `\e`, `.multiline`, `.singleline`, `.dump --preserve-rowids`, progress, pager, `.open --nofollow`, `.import` |
| Parity scripts | `DUCKDB_REF_SHELL=/opt/homebrew/bin/duckdb DUCKDB_RUST_SHELL=./target/debug/duckdb_cli bash rust_cli/diff_shells.sh rust_cli/parity_smoke.sql` and the other four parity SQL files |
| macOS package | `DUCKDB_VENDOR_VERSION=1.5.3 bash rust_cli/package.sh` |
| macOS package smoke | Extract `rust_cli/dist/duckdb-rust-cli-1.5.3-macos-arm64.tar.gz`, run `duckdb -version`, run `duckdb -c "select 42"` |
| Linux arm64 package | Docker package build and smoke using the 1.5.3 Linux arm64 library |
| Linux x86_64 package | Docker x86_64 package build and smoke using the 1.5.3 Linux amd64 library |

## Verification Record

Recorded on 2026-05-25 from branch `rust-cli`.

| Check | Result |
|---|---|
| `git branch --show-current` | `rust-cli` |
| `CARGO_TARGET_DIR=/Users/nico/Code/duckdb-rust-cli/target DUCKDB_VENDOR_VERSION=1.5.3 DUCKDB_LIB_SOURCE=repo cargo test -p duckdb_cli` | Pass, `10 passed` |
| Focused new regressions for pager, `.once`/`.output`, `.dump`, `-ui`, and interactive highlighting | Pass, `7 passed` |
| Logical-type/logging focused coverage | Pass, `11 passed` |
| `bash rust_cli/run_shell_tests.sh -rs` | Pass, `509 passed, 2 skipped` |
| Full-suite skips | `test_http_logging.py::test_http_logging_file` and `test_shell_basics.py::test_eqp`, both documented as official 1.5.3 non-applicable behavior |
| Five `rust_cli/diff_shells.sh` parity SQL scripts | Pass with transcript and exit-code comparison |
| macOS arm64 package | Pass, produced `rust_cli/dist/duckdb-rust-cli-1.5.3-macos-arm64.tar.gz` |
| macOS arm64 package smoke | Pass, `duckdb -version` prints `v1.5.3 (Variegata) 14eca11bd9`; `duckdb -c "select 42 as answer;"` returns `42` |
| Linux arm64 package | Pass in Docker linux/arm64, produced `/tmp/duckdb-rust-cli-linux-package-out/duckdb-rust-cli-1.5.3-linux-aarch64.tar.gz` |
| Linux arm64 package smoke | Pass, `duckdb -version` prints `v1.5.3 (Variegata) 14eca11bd9`; `duckdb -c "select 42 as answer;"` returns `42` |
| Linux x86_64 package | Pass in Docker linux/amd64, produced `/tmp/duckdb-rust-cli-linux-x86-package-out/duckdb-rust-cli-1.5.3-linux-x86_64.tar.gz` |
| Linux x86_64 package smoke | Pass, `duckdb -version` prints `v1.5.3 (Variegata) 14eca11bd9`; `duckdb -c "select 42 as answer;"` returns `42` |

## Progress Log

| Date | Change | Verification |
|---|---|---|
| 2026-05-25 | Rejected Rust-only `.dump --preserve-rowids` with the official-compatible error path | `test_dump.py::test_dump_rejects_preserve_rowids` passed |
| 2026-05-25 | Fixed interactive `\e` by making the linenoise completeness shim return the standalone alias to Rust before continuation rendering | `test_interactive_startup.py::test_interactive_edit_escape_alias_opens_editor_and_executes` passed; official `/opt/homebrew/bin/duckdb` also passes this test |
| 2026-05-25 | Wired `.multiline` and `.singleline` to `linenoiseSetMultiLine`; confirmed this is physical line editing mode, not semicolon completeness | `test_interactive_startup.py` and `test_interactive_statement_splitting.py` passed |
| 2026-05-25 | Added exit-status comparison to `rust_cli/diff_shells.sh` | All five parity SQL scripts passed against `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Re-ran the shell suite with skip reporting | `498 passed, 2 skipped`; skips are `test_http_logging.py::test_http_logging_file` and `test_shell_basics.py::test_eqp` |
| 2026-05-25 | Classified the two shell skips as stale/non-applicable for DuckDB 1.5.3 parity and updated skip reasons | Official 1.5.3 also rejects `.eqp`; official 1.5.3 reports `http_logging_output` as deprecated and unusable |
| 2026-05-25 | Re-ran Rust test target before adding unit coverage | `cargo test -p duckdb_cli` passed with 0 tests |
| 2026-05-25 | Closed progress-bar P1 by disabling the default engine progress print renderer that official 1.5.3 did not emit in PTY testing | `test_prompt.py::test_progress_bar_does_not_print_default_engine_renderer` passes against Rust and `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Matched `-version` and `.version` detail output, including codename/source id and compiler line | `test_command_line_arguments.py::test_version` and `test_dot_version` pass against Rust and `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Added `.open --nofollow` symlink coverage matching official no-op/follow behavior | `test_shell_basics.py::test_open_nofollow_accepts_symlink_like_official` passes against Rust and `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Matched `.import` dotted-table behavior to official literal table names and added generic CSV parameter coverage | `test_import.py::test_import_dotted_table_name_appends_like_official` and `test_import_csv_generic_parameters` pass against Rust and `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Added focused Rust unit tests for prompt parsing, progress templates, display color sorting, and DuckBox JSON formatting | `cargo test -p duckdb_cli` passes with 10 tests |
| 2026-05-25 | Fixed DuckBox automatic pager triggering for interactive row-threshold output | `test_pager.py::test_duckbox_automatic_pager_uses_threshold` passes against Rust and `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Added deterministic `.once`/`.output` reset and BOM coverage | New shell basics tests pass against Rust and `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Added `.dump` catalog edge coverage for constraints/views/indexes and documented official sequence/macro omissions | New dump catalog test passes against Rust and `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Added controlled `.ui_command` + `-ui` launch-path coverage | New UI command test passes against Rust and `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Added interactive SQL highlighting smoke and re-ran logical-type/logging focused coverage | Highlight smoke passes against Rust and `/opt/homebrew/bin/duckdb`; variant/nested JSON/logging tests pass |
| 2026-05-25 | Re-ran full shell suite after all code/test changes | `509 passed, 2 skipped` |
| 2026-05-25 | Re-ran all five transcript parity scripts with exit-status comparison | All five passed against `/opt/homebrew/bin/duckdb` |
| 2026-05-25 | Built and smoked macOS arm64 package | Package reports `v1.5.3 (Variegata) 14eca11bd9` and returns `42` |
| 2026-05-25 | Built and smoked Linux arm64 package in Docker | Package reports `v1.5.3 (Variegata) 14eca11bd9` and returns `42` |
| 2026-05-25 | Built and smoked Linux x86_64 package in Docker | Package reports `v1.5.3 (Variegata) 14eca11bd9` and returns `42` |

## Definition Of Done

| Requirement | Status |
|---|---|
| Every P1 behavior delta is fixed or explicitly documented as intentionally out of scope | Done for current P1 list: B1 confirmed compatible; B2/B3/B4/B6 fixed |
| Every P1 verification gap is closed or explicitly documented as intentionally out of scope | Done for current P1 list: V1/V2/V3 documented; V4 fixed |
| Full shell suite passes with expected skip count and documented skip reasons | Done: `509 passed, 2 skipped`; skip reasons updated for 1.5.3 |
| Parity scripts compare both transcript and exit code | Done |
| macOS arm64 package builds and smoke-tests | Done |
| Linux arm64 package builds and smoke-tests | Done |
| Linux x86_64 package is verified or explicitly scoped out | Done: verified in Docker linux/amd64 |
| Windows behavior | Done: explicitly out of scope |
| macOS signing/notarization | Done: explicitly out of scope as distribution hardening |
| `GOAL.md` is updated with final status before commit | Done |
| Completed changeset is committed after verification | Done in the final commit containing this file |

## Source Findings

The inventory used these findings files:

- `/Users/nico/.agents/findings/steady-inventory-duckdb-cli.md`
- `/Users/nico/.agents/findings/steady-inventory-duckdb-cli-agent-commands.md`
- `/Users/nico/.agents/findings/steady-inventory-duckdb-cli-agent-rendering.md`
- `/Users/nico/.agents/findings/steady-inventory-duckdb-cli-agent-interactive.md`
- `/Users/nico/.agents/findings/steady-inventory-duckdb-cli-agent-tests.md`
