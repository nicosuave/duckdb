# DuckDB Rust CLI Strict 1.5.3 Parity Goal

Date: 2026-05-26

Branch: `rust-cli`

Reference CLI: DuckDB `v1.5.3`

Baseline commit: `2f4313c769 Complete Rust CLI 1.5.3 parity follow-ups`

Goal status: complete

## Objective

The previous goal completed the documented macOS/Linux DuckDB `1.5.3` CLI parity scope. This goal is the stricter follow-up: inventory every remaining shortcoming against the official DuckDB `1.5.3` CLI, implement the locally actionable deltas, and leave only explicitly verified or credential/runtime-gated items open.

Full parity answer for the approved scope: yes. Windows behavior and macOS notarization were explicitly approved out of scope by the user on 2026-05-26. The remaining locally verifiable macOS/Linux surface has been implemented and verified: package re-smokes, UI extension launch gating, terminal/editor highlighting parity, DuckBox rendering deltas, and the official 1.5.3 shell test surface.

## Current Baseline

| Check | Baseline result |
|---|---|
| Runtime target | Rust CLI reports DuckDB `v1.5.3` |
| Prior parity commit | `2f4313c769 Complete Rust CLI 1.5.3 parity follow-ups` |
| Prior shell suite | `509 passed, 2 skipped` |
| Prior Rust tests | `10 passed` |
| Prior packages | macOS arm64, Linux arm64, Linux x86_64 built and smoke-tested |
| Prior explicit scope | macOS/Linux CLI parity, not absolute parity |
| Active goal tool objective | Implement or explicitly resolve the strict parity leftovers listed in this file |

## Shortcomings Inventory

| ID | Area | Official DuckDB 1.5.3 behavior | Rust CLI status | Required work | Verification |
|---|---|---|---|---|---|
| S1 | Windows build/link support | Official CLI ships Windows artifacts and Windows-specific shell code | Some scaffolding exists, but Windows is no longer required for this goal | Approved out of scope by user on 2026-05-26 | No goal-blocking verification required |
| S2 | Windows package layout | Official Windows distribution uses `duckdb.exe` plus `duckdb.dll`/import libs as appropriate | Some package-script support exists, but Windows is no longer required for this goal | Approved out of scope by user on 2026-05-26 | No goal-blocking verification required |
| S3 | Windows terminal detection | Official uses platform console APIs | Portable `std::io::IsTerminal` is used, but Windows runtime proof is no longer required | Approved out of scope by user on 2026-05-26 | No goal-blocking verification required |
| S4 | Windows shell/pager/open commands | Official uses Windows command handling where needed | Some Windows command handling exists, but Windows is no longer required for this goal | Approved out of scope by user on 2026-05-26 | No goal-blocking verification required |
| S5 | Windows Ctrl-C | Official installs a Windows console control handler | Some Windows handler scaffolding exists, but Windows is no longer required for this goal | Approved out of scope by user on 2026-05-26 | No goal-blocking verification required |
| S6 | Windows `.utf8` | Official exposes `.utf8` on Windows | Windows-only command exists, but Windows is no longer required for this goal | Approved out of scope by user on 2026-05-26 | Verify only that it stays hidden on macOS/Linux |
| S7 | Windows home/history/extension paths | Official resolves Windows home paths | Portable home fallback exists, but Windows runtime proof is no longer required | Approved out of scope by user on 2026-05-26 | No goal-blocking verification required |
| S8 | Windows binary/text output | Official C shell can use `.binary` with CRT text/binary modes | Windows exact text/binary behavior is no longer required for this goal | Approved out of scope by user on 2026-05-26 | No goal-blocking verification required |
| S9 | DuckBox zero-row rendering | Official closes the box after header/type rows and prints `0 rows` outside the box | Fixed for regular DuckBox and `.columns` zero-row results | Keep regression coverage | `test_duckbox_zero_rows_footer` against Rust and official |
| S10 | DuckBox `.columns` zero-row pivot | Official does not pivot zero-row results into `Column`/`Type` rows | Fixed as part of S9 | Keep regression coverage | `.columns; select ... where false` exact output |
| S11 | Nested DuckBox highlighting | Official annotates nested keys and NULL spans using `string_constant` and `null_value` styles | Rust now highlights quoted nested keys and case-preserving `NULL`/`null` spans | Expand if future official spans include more value classes | `test_nested_duckbox_value_highlight` plus official comparison |
| S12 | Full result/style highlighting matrix | Official has many style elements: layout, footer, metadata, logs, prompt, error emphasis, suggestions, etc. | Done for the Rust-rendered surfaces: layout/footer/table metadata now use the highlight style table, custom `layout`/`footer` snapshots match official, and unsupported/unrendered suggestion/log surfaces are not exposed by this Rust CLI path | Keep regression coverage for every rendered style element that is used locally | `test_custom_layout_and_footer_highlight`; focused highlighting suite against Rust and official |
| S13 | Interactive SQL highlighting exact style parity | Official linenoise uses parser tokenization and full `ShellHighlight` style table | Done: `.highlight`, `.highlight_mode`, and `.highlight_colors` sync into the vendored linenoise shim, with official default style table parity for the editor path | Keep deterministic PTY snapshots | PTY tests for `.highlight_colors keyword red` and `.highlight off` against Rust and official |
| S14 | Real UI extension launch | Official `-ui` runs `CALL start_ui()` via the UI extension | Done: existing failing-command wiring remains covered, and a gated real UI extension server launch smoke passes with `DUCKDB_UI_SMOKE=1` | Keep the real extension smoke gated because it downloads/loads the UI extension | `DUCKDB_UI_SMOKE=1 test_ui_command_start_ui_server_smoke` against Rust and official |
| S15 | macOS package rpaths | Official distribution should not depend on a developer checkout | `package.sh` now removes repo-local rpaths from the packaged binary when possible | Verify no absolute repo-local rpaths in packaged Mach-O | `otool -l duckdb` rejects `/Users/nico/Code/duckdb-rust-cli` and vendor/build rpaths |
| S16 | Package checksums | Release artifacts should have checksums | `package.sh` now writes `.sha256` when `shasum` or `sha256sum` exists | Verify checksum file is emitted and validates | `shasum -a 256 -c <archive>.sha256` or `sha256sum -c` |
| S17 | macOS signing/notarization | Requires Developer ID signing and Apple notarization for hardened distribution | Script has env-gated signing hooks; no credentials are available in this environment | Approved out of scope by user on 2026-05-26 | No goal-blocking verification required |
| S18 | Linux package rpaths | Official-style package should run from extracted directory without repo-local dependencies | Done: `package.sh` normalizes Linux package `RUNPATH` to `$ORIGIN:$ORIGIN/../lib` with `patchelf` | Keep Docker package smokes in release verification | Docker linux/amd64 and linux/arm64 package smokes, `patchelf --print-rpath`, and `readelf -d` |
| S19 | Official test-suite drift | DuckDB CLI tests can change after 1.5.3 or when local tests are extended | Done: final full shell suite pass is `515 passed, 3 skipped` | Keep skip reasons explicit | `bash rust_cli/run_shell_tests.sh -rs` |
| S20 | Newly discovered 1.5.3 gaps | The stricter audit can still find edge deltas | Done for this pass: no remaining locally actionable macOS/Linux 1.5.3 CLI gaps are known after the final suite, parity scripts, UI smoke, and package smokes | Reopen inventory if a new official-vs-Rust delta is found | Source finding notes and focused official-vs-Rust probes |

## Implementation Plan

| Step | Work | Status |
|---|---|---|
| 1 | Record strict full-parity answer and active inventory in this file | Done |
| 2 | Resolve Windows build/runtime scope | Done: user approved skipping Windows on 2026-05-26 |
| 3 | Fix newly discovered DuckBox and nested-highlight deltas | Done |
| 4 | Harden package script with Windows names, macOS rpath cleanup, checksum output, and signing hooks | Done for local script changes |
| 5 | Add regression tests for every fixed behavior | Done for DuckBox/highlighting deltas |
| 6 | Re-run focused tests, Rust tests, full shell suite, parity scripts, and package smokes | Done |
| 7 | Update verification record and either complete the goal or leave explicit external blockers | Done |
| 8 | Commit only when the goal is complete or remaining items are approved as out of scope | Ready: all approved-scope work is complete and verified |

## Verification Matrix

Run from `/Users/nico/Code/duckdb-rust-cli`.

| Check | Command |
|---|---|
| Branch/status | `git status --short --branch` |
| Format | `cargo fmt --check` |
| Rust build | `cargo build -p duckdb_cli` |
| Rust tests | `cargo test -p duckdb_cli` |
| Focused DuckBox/highlighting tests | `uv run pytest tools/shell/tests/test_duckbox_complex_value_rendering.py tools/shell/tests/test_highlighting.py --shell-binary /Users/nico/.cargo/shared-target/debug/duckdb_cli` |
| Full shell suite | `bash rust_cli/run_shell_tests.sh -rs` |
| Parity scripts | `DUCKDB_REF_SHELL=/opt/homebrew/bin/duckdb DUCKDB_RUST_SHELL=/Users/nico/.cargo/shared-target/debug/duckdb_cli bash rust_cli/diff_shells.sh <script>` for all parity SQL scripts |
| macOS package | `DUCKDB_VENDOR_VERSION=1.5.3 bash rust_cli/package.sh` |
| macOS package rpath audit | `otool -l <extracted>/duckdb` and reject repo-local rpaths |
| macOS package smoke | `<extracted>/duckdb -version` and `<extracted>/duckdb -c "select 42"` |
| Linux arm64 package | Existing Docker linux/arm64 package smoke |
| Linux x86_64 package | Existing Docker linux/amd64 package smoke |
| Windows build | Not required for this goal; user approved skipping Windows on 2026-05-26 |
| Windows runtime smoke | Not required for this goal; user approved skipping Windows on 2026-05-26 |
| UI smoke, optional | `DUCKDB_UI_SMOKE=1` with UI extension available |
| Signing/notarization | Not required for this goal; user approved skipping notarization on 2026-05-26 |

## Verification Record

| Date | Check | Result |
|---|---|---|
| 2026-05-26 | `cargo check -p duckdb_cli` | Pass with existing warnings |
| 2026-05-26 | `cargo build -p duckdb_cli` | Pass with existing warnings |
| 2026-05-26 | `cargo test -p duckdb_cli` | Pass, `10 passed` with existing warnings |
| 2026-05-26 | Focused DuckBox/highlighting tests | Pass, `6 passed` |
| 2026-05-26 | Focused empty-result/DuckBox/highlighting slice | Pass, `7 passed` |
| 2026-05-26 | Full shell suite | Pass, `511 passed, 2 skipped` |
| 2026-05-26 | Five transcript parity scripts against `/opt/homebrew/bin/duckdb` | Pass with matching output and exit status |
| 2026-05-26 | `bash -n rust_cli/package.sh` | Pass |
| 2026-05-26 | macOS arm64 package build | Pass, produced `rust_cli/dist/duckdb-rust-cli-1.5.3-macos-arm64.tar.gz` and `.sha256` |
| 2026-05-26 | macOS arm64 package smoke | Pass, `duckdb -version` reports `v1.5.3 (Variegata) 14eca11bd9`; `select 42` returns `42`; checksum validates |
| 2026-05-26 | macOS package rpath audit | Pass, packaged binary has `@executable_path` only for `LC_RPATH`; no repo-local vendor/build rpath remains |
| 2026-05-26 | Windows and notarization scope | Approved out of scope by user |
| 2026-05-26 | Focused editor/style/UI/macOS/Linux visibility slice | Pass, `21 passed` |
| 2026-05-26 | Focused PTY editor highlighting tests against official `/opt/homebrew/bin/duckdb` | Pass, `2 passed` |
| 2026-05-26 | Focused layout/footer highlight test against official `/opt/homebrew/bin/duckdb` | Pass, `1 passed` |
| 2026-05-26 | Non-Windows `.utf8` visibility test against official `/opt/homebrew/bin/duckdb` | Pass, `1 passed` |
| 2026-05-26 | Gated UI extension smoke against Rust CLI | Pass with `DUCKDB_UI_SMOKE=1`, `1 passed` |
| 2026-05-26 | Gated UI extension smoke against official `/opt/homebrew/bin/duckdb` | Pass with `DUCKDB_UI_SMOKE=1`, `1 passed` |
| 2026-05-26 | Final `cargo test -p duckdb_cli` | Pass, `10 passed` with existing warnings |
| 2026-05-26 | Final full shell suite | Pass, `515 passed, 3 skipped` |
| 2026-05-26 | Final five transcript parity scripts against `/opt/homebrew/bin/duckdb` | Pass with matching output and exit status |
| 2026-05-26 | Final macOS arm64 package smoke | Pass, version/query/checksum OK; rpath is only `@executable_path` |
| 2026-05-26 | Final Linux x86_64 package smoke | Pass in Docker linux/amd64; checksum OK, version/query OK, `RUNPATH` is `$ORIGIN:$ORIGIN/../lib` |
| 2026-05-26 | Final Linux arm64 package smoke | Pass in Docker linux/arm64; checksum OK, version/query OK, `RUNPATH` is `$ORIGIN:$ORIGIN/../lib` |

## Progress Log

| Date | Change | Verification |
|---|---|---|
| 2026-05-26 | Added portable home-directory helper for `HOME`, `USERPROFILE`, and `HOMEDRIVE` + `HOMEPATH` | `cargo build -p duckdb_cli` |
| 2026-05-26 | Replaced raw Unix `isatty` with `std::io::IsTerminal` | `cargo build -p duckdb_cli` |
| 2026-05-26 | Added Windows shell command handling, default pager, spreadsheet opener path, Ctrl-C handler, and Windows-only `.utf8` metadata command | `cargo build -p duckdb_cli`; Windows runtime verification approved out of scope |
| 2026-05-26 | Generalized C++ build scripts for MSVC/GNU Windows static libs while preserving macOS/Linux builds | `cargo build -p duckdb_cli` on macOS |
| 2026-05-26 | Added Windows package names and library names to `package.sh` | `bash -n rust_cli/package.sh`; Windows package smoke approved out of scope |
| 2026-05-26 | Fixed DuckBox zero-row footer rendering and `.columns` zero-row behavior | Focused pytest, `6 passed` |
| 2026-05-26 | Added nested DuckBox key/NULL highlighting annotations | Focused pytest, `6 passed` |
| 2026-05-26 | Updated stale empty-result test expectation to official 1.5.3 zero-row DuckBox output | Focused pytest, `7 passed`; full suite `511 passed, 2 skipped` |
| 2026-05-26 | Added package checksum output, macOS package rpath cleanup, and env-gated signing hooks | `bash -n rust_cli/package.sh`; macOS package build/smoke/checksum/rpath audit passed |
| 2026-05-26 | Closed Windows runtime/build and macOS notarization as required gates | User explicitly approved skipping Windows and notarization |
| 2026-05-26 | Bridged `.highlight`, `.highlight_mode`, and `.highlight_colors` into vendored linenoise editor highlighting | Focused PTY tests pass against Rust and official |
| 2026-05-26 | Routed layout/footer/table metadata rendering through the shared highlight style table | `test_custom_layout_and_footer_highlight` passes against Rust and official |
| 2026-05-26 | Added gated real UI extension smoke using `-ui` plus `.ui_command start_ui_server()` | `DUCKDB_UI_SMOKE=1` focused test passes against Rust and official |
| 2026-05-26 | Added macOS/Linux `.utf8` hidden-command regression | Focused test passes against Rust and official |
| 2026-05-26 | Normalized Linux package `RUNPATH` with `patchelf` | Final Docker linux/amd64 and linux/arm64 package smokes pass |
| 2026-05-26 | Re-ran final full shell suite and parity scripts | `515 passed, 3 skipped`; all five transcript parity scripts pass |

## Definition Of Done

| Requirement | Status |
|---|---|
| All locally actionable strict-parity behavior gaps fixed | Done for approved macOS/Linux scope |
| Windows build and runtime behavior either verified or explicitly blocked by lack of Windows host/CI | Done: explicitly approved out of scope by user |
| Real UI launch either verified with `DUCKDB_UI_SMOKE=1` or explicitly left runtime-gated | Done: gated UI server smoke passes against Rust and official |
| Exact editor highlighting either implemented and tested or explicitly approved as out of scope | Done: linenoise bridge plus PTY tests pass |
| macOS/Linux packages rebuilt and smoked after package script changes | Done: macOS arm64, Linux x86_64, and Linux arm64 pass |
| Full shell suite rerun after current changes | Done: `515 passed, 3 skipped` |
| `GOAL.md` final status updated before commit | Done |
| Commit created only after completion or approved scope decision | Ready to commit after final git review |

## Source Findings

| Source | Coverage |
|---|---|
| Windows audit subagent | Build scripts, package layout, terminal detection, shell/pager/open behavior, Ctrl-C, `.utf8`, home paths |
| DuckBox audit subagent | Zero-row `.columns`, nested/JSON highlighting annotations, stale column-pruning concern |
| UI/signing/highlighting audit subagent | `-ui` wiring, real UI launch, macOS rpaths/signing/notarization, interactive/result highlighting gaps |
