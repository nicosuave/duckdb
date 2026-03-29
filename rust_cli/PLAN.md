# DuckDB CLI reimplementation in Rust (macOS/Linux)

Goal: a drop-in compatible `duckdb` CLI reimplemented in Rust, linking to a shipped `libduckdb` (DuckDB **1.4.3** first), matching dot-command surface area, output formatting quirks, and startup/rc behavior.

Non-goals (initially): Windows support.

Acceptance: run `tools/shell/tests/` against the Rust CLI with matching stdout/stderr/exit codes.

Status (as of 2026-01-21, current workspace state):
- Interactive mode uses vendored linenoise (Ctrl-R reverse search + tab completion callback).
- `bash rust_cli/run_shell_tests.sh` => **487 passed, 2 skipped** (builds with `DUCKDB_LIB_SOURCE=repo` + runs with `DUCKDB_SKIP_LIB_VERSION_CHECK=1`).
- `EXPLAIN` statements render via the dedicated EXPLAIN renderer (not the `explain_key/explain_value` table).
- `DESCRIBE` statements render via the table-metadata renderer (not the raw `column_name/column_type/...` table).
- `.render_completion` / `.render_errors` are supported (wired to linenoise render toggles).
- Added PTY-backed tests for interactive startup banner, multiline prompt behavior, `~/.duckdbrc` loading-resources message, and Ctrl-R reverse search.
- Added PTY-backed test for interactive Ctrl-C double-press hint message.
- Added PTY-backed test for interactive Ctrl-D exit behavior.
- Duckbox: avoid expanding nested values in maxwidth mode unless needed; added regression coverage.
- Duckbox: added JSONFormatter-style nested pretty-printing (partial port) and regression coverage.
- Duckbox: fixed truncation marker alignment in numeric columns; added regression coverage.
- Duckbox: fixed nested JSON type alias rendering in struct type rows; added regression coverage.
- Added exact-output tests for `.mode` across key modes (`tools/shell/tests/test_mode_exact_small.py`, `tools/shell/tests/test_mode_exact_quoting.py`).
- Added statement-splitting edge case coverage for `;` inside strings/comments (`tools/shell/tests/test_statement_splitting_edgecases.py`).
- Added regression coverage for `-echo` multi-statement echo slice behavior (`tools/shell/tests/test_echo_multistatement_tail.py`).
- Added `.mode` invalid-mode error/exit behavior coverage (`tools/shell/tests/test_mode_invalid.py`).
- Added PTY-backed test for linenoise Ctrl-A/Ctrl-E editing (`tools/shell/tests/test_interactive_ctrl_a_ctrl_e.py`).
- Added PTY-backed tests for history navigation (Up arrow + Ctrl-P) (`tools/shell/tests/test_interactive_history_navigation.py`).
- Added PTY-backed tests for Ctrl-U/Ctrl-K editing (`tools/shell/tests/test_interactive_ctrl_u_ctrl_k.py`).
- Added markdown pipe-escaping coverage (`tools/shell/tests/test_mode_markdown_pipe_escape.py`).
- Added JSON typed-value rendering coverage for `.mode json/jsonlines` (`tools/shell/tests/test_mode_json_typed_json.py`).
- Added newline-in-value coverage across modes (`tools/shell/tests/test_mode_newlines.py`).
- Added Unicode render-width coverage for `.mode box` (`tools/shell/tests/test_mode_box_unicode_width.py`).
- Added Unicode render-width coverage for `.mode column` (`tools/shell/tests/test_mode_column_unicode_width.py`).
- Added Unicode render-width coverage for `.mode duckbox` ZWJ emoji width (👩‍💻) (`tools/shell/tests/test_duckbox_unicode_width_zwj.py`).
- Fixed combined `.decimal_sep`/`.thousand_sep` formatting (`tools/shell/tests/test_readable_numbers.py`).
- Fixed `.large_number_rendering` invalid-bool stderr text + defaulting (`tools/shell/tests/test_readable_numbers.py`).
- Duckbox: `.thousand_sep` does not apply to signed numerics (shell quirk) (`tools/shell/tests/test_readable_numbers.py`).
- Duckbox: single-row FOOTER mode prints readable footer row even after streaming->non-streaming fallback (`tools/shell/tests/test_readable_numbers.py`).
- Duckbox: single-row FOOTER mode centers the numeric value row (shell quirk) (`tools/shell/tests/test_readable_numbers.py`).
- Duckbox: VARIANT type row parity in duckbox mode, even when the C API reports unknown/invalid (describe-based type patching) (`tools/shell/tests/test_duckbox_variant_type_row.py`).
- Fixed `.databases` rendering via table-metadata renderer (`tools/shell/tests/test_databases_exact.py`).
- Fixed complex/nested value normalization edge cases (typed literals in arrays + struct-key single-quote escaping).
- Added regression coverage for complex/nested value rendering (`tools/shell/tests/test_duckbox_complex_value_rendering.py`, `tools/shell/tests/test_complex_value_struct_key_quotes.py`).
- Added PTY-backed test for Ctrl-W word delete (`tools/shell/tests/test_interactive_ctrl_w.py`).
- Made interactive Ctrl-R reverse search robust under PTY timing (stdin non-canonical between commands).
- Added regression coverage for TIMESTAMPTZ local offset with `SET TimeZone` (`tools/shell/tests/test_timestamptz_local_offset.py`).
- Added regression coverage for duckbox JSON quoting context (`tools/shell/tests/test_duckbox_json_quoting_context.py`).
- Added regression coverage for `.display_colors` output (`tools/shell/tests/test_display_colors.py`).
- Added regression coverage that `.help` output is plain (no ANSI sequences) (`tools/shell/tests/test_help_no_ansi.py`).
- Added regression coverage for `.highlight_mode` invalid-usage (`tools/shell/tests/test_highlight_mode_invalid.py`).
- Added regression coverage for `.last` (duckbox untruncation) (`tools/shell/tests/test_last_duckbox.py`).
- Fixed `.schema --indent` formatting threshold + double-semicolon output; added regression coverage (`tools/shell/tests/test_shell_basics.py`).
- Fixed `.schema` schema-qualified pattern bug parity (`.schema s.%` binder error + abort in `-f` mode); added regression coverage (`tools/shell/tests/test_schema_schema_qualified_abort.py`).
- Fixed deprecated highlight-color aliases (`.keyword`/`.comment`/etc.) warning + unknown-color “did you mean” output parity; added regression coverage (`tools/shell/tests/test_deprecated_highlight_color_aliases.py`).
- Fixed `.highlight_mode auto` parity and `.highlight_colors` extended color names parity; added regression coverage (`tools/shell/tests/test_highlight_mode_invalid.py`, `tools/shell/tests/test_highlight_colors_extended.py`).
- Fixed `.dump` ordering/semicolons parity (interleave table DDL+data, views last, schema emits `;;`); added regression coverage (`tools/shell/tests/test_dump.py`).

## Phase 0: Ground rules (pin + shim scope)
- [x] Pin DuckDB version to target. (1.4.3)
- [x] Decide distribution layout for shipped `libduckdb` (macOS `.dylib`, Linux `.so`) and header.
- [x] Define strict runtime version check behavior (error text + exit code).
- [x] Define minimal shim API surface (only where C API cannot match shell semantics):
  - [x] Cast-to-varchar display semantics (no quotes/typed-literals, correct TIMESTAMPTZ offset) without needing DuckDB’s internal C++ APIs.
  - [x] DuckBox renderer parity (duckdb internal box renderer behavior + formatting knobs).
  - [x] Optional: exact display width computation parity (linenoise render width).

## Phase 1: Repo skeleton (standalone Cargo)
- [x] Create `crates/duckdb_cli/` Rust binary crate.
- [x] Create `crates/duckdb_sys/` FFI crate for the pinned `duckdb.h`.
  - [x] Bindgen strategy (generated bindings committed for pinned version, no network fetch at build).
- [x] Create `crates/duckdb_shellshim/` minimal C++ shim + `build.rs`.
  - [x] Shim headers and stable C ABI exported functions (used for `.echo` slice parity).
  - [x] Keep `duckdb_shellshim` only for `.echo` slice parity (builds `echo.cc` only, avoids DuckDB internal C++ dependencies).
- [x] Add a top-level `Cargo.toml` workspace.
- [x] Add CI scripts/notes for macOS + Linux builds (no Windows).

## Phase 2: Parity-first shell state + option parsing (no REPL yet)
Source of truth: `tools/shell/shell.cpp`, `tools/shell/shell_command_line_option.cpp`, `tools/shell/include/shell_state.hpp`.

- [x] Implement shell state struct mirroring required fields (mode, separators, headers, startup_text/safe_mode stubs, etc.).
- [x] Implement option parsing with exact behaviors:
  - [x] Underscore-to-dash legacy behavior for options.
  - [x] “Did you mean …” candidate error text.
  - [x] Two-pass callbacks: pre-init, rc load (stub), post-init overrides.
  - [x] Exit codes match (help/version/parse errors).

## Phase 3: `.duckdbrc` + startup text semantics
- [x] Implement `~/.duckdbrc` discovery logic (+ `-init` override).
- [x] Implement rc processing modes (DUCKDB_RC vs FILE behavior) (minimal execution).
- [x] Implement `.startup_text` semantics and the “Loading resources from …” message gating (minimal).
- [x] Implement interactive startup banner/version line behavior (via shim).
- [x] Ensure TIMESTAMPTZ renders in local time zone by default (best-effort `LOAD icu` + `SET TimeZone` + render-time local offset formatting).

## Phase 4: Meta-command (dot-command) framework
Source of truth: `tools/shell/shell_metadata_command.cpp` and `ShellState::DoMetaCommand` parsing in `tools/shell/shell.cpp`.

- [x] Implement `.command` line tokenizer (quotes/backslashes) matching current behavior.
- [x] Implement metadata command table (Rust `DOT_COMMANDS`) with prefix matching + `match_size` rules.
- [x] Implement unknown-command messaging parity (including “did you mean” behavior) and invalid-usage formatting.
- [x] Implement `.help` parity:
  - [x] `.help` prints one-liners,
  - [x] `.help --all` prints full blocks,
  - [x] `.help` output is generated from `DOT_COMMANDS` + per-command help metadata (no `HELP_LINES` list in `exec.rs`).
  - [x] `.help PATTERN` prints full command blocks for matches (prefix match + LIKE substring behavior).

## Phase 5: SQL execution pipeline (C API)
- [x] Open database + connect using C API with config knobs needed for shell options.
- [x] Implement query execution:
  - [x] Multi-statement handling equivalent to shell (see `SQLIsComplete` + semicolon behavior).
  - [x] Basic single-statement execution (materialized `duckdb_query`) + basic rendering for `csv|list|line|trash`.
  - [x] DuckBox path uses `duckdb_extract_statements` + `duckdb_execute_prepared` for multi-statement execution.
  - [x] Suppress non-query results (`duckdb_result_return_type`), and track/print `.changes` (avoid rendering `Count` result tables for DDL/DML).
  - [x] Duckbox streaming execution using `duckdb_execute_prepared_streaming` + `duckdb_fetch_chunk` (currently enabled for `.maxrows -1`, rows mode only).
  - [x] Streaming execution for non-duckbox modes (`duckdb_execute_prepared_streaming` + chunk API renderers).
  - [x] Interrupt handling via SIGINT -> `duckdb_interrupt(connection)` and reset semantics matching `ShellState::ClearInterrupt` (count reset between prompts).
- [x] Implement `.open` options including `--new`, `--nofollow`, `--readonly`, `--sql`.

## Phase 6: Render modes (pure Rust + shim where required)
Source of truth: `RenderMode` enum in `tools/shell/include/shell_state.hpp` and renderers in `tools/shell/shell_renderer.cpp`.

- [x] Implement per-mode renderer selection matching `.mode` behavior and errors.
- [x] Implement value materialization strategy matching shell:
  - [x] Avoid DuckDB `Value::ToString` typed-literals for common scalars (varchar/date/time/timestamp).
  - [x] Format `timestamp with time zone` with local offset (macOS/Linux via `localtime_r`).
- [x] Make TIMESTAMPTZ rendering honor `SET TimeZone` changes (sync process `TZ` from `current_setting('TimeZone')`).
- [x] Implement output modes and exact formatting:
  - [x] `csv`, `tabs`, `list`, `line`, `column`, `table`, `box`, `markdown`, `html`, `latex`, `insert`, `json`, `jsonlines`, `ascii`, `trash`.
  - [x] `duckbox` parity:
    - [x] Streaming output when rendering all rows (no full-result buffering).
    - [x] Analyze-window truncation (`.maxrows -1 ANALYZE`) with `…`.
    - [x] `.maxwidth` wrap behavior (bounded by `.maxrows` vertical budget, matches “still truncates if too long”).
    - [x] `.large_number_rendering` `footer|all` (value row + readable footer row + row-count footer).
    - [x] Control-character escaping (`\n`, etc.) for values + column names.
    - [x] `.columns` (duckbox pivot) full parity (Row N headers, footer semantics).
    - [x] Column pruning/splitting when too wide (including exact “…” column behavior).
- [x] Implement `.nullvalue`, `.separator`, `.width`, `.headers`, `.columns/.rows`, `.maxrows`, `.maxwidth`, `.decimal_sep`, `.thousand_sep`, `.large_number_rendering` (core toggles wired; some per-mode quirks still missing).
- [x] Implement pager behavior (`.pager`) and apply to non-duckbox output paths (TTY-gated like shell).

## Phase 7: Remaining dot-commands (full surface area)
- [x] Categorize commands into:
  - [x] State-only commands.
  - [x] SQL-backed commands (information_schema/sqlite_schema queries).
  - [x] OS-backed commands (guarded by safe_mode): `.cd`, `.system/.shell`, `.output/.once`, `.edit`, etc.
- [x] Implement each command to match output and exit behavior.
- [x] Implement `.dump` behavior and options (`--newlines`) with exact output (tests-covered subset).

## Parity gap checklist (from current findings)
- [x] Dot-command framework parity:
  - [x] Real metadata table parity (match_size edge cases, ordering, aliases).
  - [x] `.help --all` and `.help PATTERN` exact block behavior.
  - [x] Unknown-command + “did you mean” messaging parity.
- [x] Dot-command surface area parity:
  - [x] `.open` flags (`--new`, `--nofollow`, `--readonly`, `--sql`).
  - [x] `.dump` (minimal: schema + data + patterns + `--newlines` for tests).
  - [x] `.schema`, `.databases`, `.indexes`, `.read` (not all output quirks verified).
  - [x] `.tables`, `.import`.
  - [x] `.output/.once/.excel`, `.cd`, `.system/.shell`, `.show`, `.timer`, `.log`.
  - [x] `.edit` (interactive-only via linenoise; batch prints “unsupported” like the shipped shell).
  - [x] `.last` (duckbox-only; prints last duckbox result without truncation).
  - [x] `.display_colors` (minimal swatch output).
  - [x] `.progress_bar` (minimal state; not fully parity-validated).
  - [x] `.ui_command` (sets UI call; not fully parity-validated).
  - [x] `.highlight_mode` (dark/light/mixed state; auto-detect best-effort via OSC 11 background query).
  - [x] State toggles: `.width`, `.maxrows`, `.maxwidth`, `.decimal_sep`, `.thousand_sep`, `.large_number_rendering`, etc.
- [x] SQL execution quirks parity:
  - [x] Exact statement splitting/semicolon behavior (ported `ShellState::SQLIsComplete` + regression coverage for tricky semicolon/comment/string/dollar-quote cases).
  - [x] Streaming execution path (C API chunk streaming; duckbox uses streaming in `.maxrows -1` rows mode).
  - [x] Ctrl-C/interrupt semantics (best-effort; not exhaustively validated across all interactive edge cases).
- [x] Render modes parity:
  - [x] box/table/markdown/json/jsonlines/insert/html/latex/column/ascii quirks.
  - [x] full duckbox parity: width/rows, number formatting, separators, null rendering.
- [x] Interactive UX parity:
  - [x] Terminal-width/render-width quirks.
  - [x] Dark/light auto-detect parity.
  - [x] Completion from metadata table + SQL completion via `CALL sql_auto_complete(...)`.
  - [x] Verify linenoise keybindings behavior (incl. reverse search) matches shipped shell.

## Known mismatches to fix next
- [x] `.help PATTERN` matches `tools/shell` showHelp() glob+LIKE behavior (prefix vs substring rules).
- [x] Duckbox: JSON/nested pretty-printing uses a JSONFormatter-style port and matches current shell output in regression coverage.

## Phase 8: Interactive REPL ergonomics (macOS/Linux)
- [x] Line editing + history:
  - [x] Match history file env var `DUCKDB_HISTORY` else `~/.duckdb_history`.
  - [x] History size (2000) and save semantics.
- [x] Completion:
  - [x] Dot command completions from metadata table.
  - [x] SQL completion via `CALL sql_auto_complete(...)` query.
- [x] Render width parity (either port linenoise width or shim it).
- [x] Highlighting controls: `.highlight_results`, `.highlight_errors`, `.highlight`, `.highlight_colors` (minimal: column_name/column_type/null_value).
- [x] Prompt parsing validation for `.prompt` (error messages + tokenization parity).

## Phase 9: Test harness + regression workflow
- [x] Wire `tools/shell/tests/` to run against the Rust binary.
- [x] Add targeted “golden transcript” tests for:
  - [x] startup/rc messaging,
  - [x] unknown option/unknown command error text,
  - [x] `.mode` formatting diffs.
- [x] Add a simple “run both shells and diff” dev helper (optional, local-only).

Notes:
- In a sandboxed environment, set `UV_CACHE_DIR` to a workspace path (e.g. `UV_CACHE_DIR=$PWD/.uv-cache`) so `uv run pytest` can write its cache. `bash rust_cli/run_shell_tests.sh` does this.

## Phase 10: Packaging (macOS/Linux)
- [x] Bundle `libduckdb` next to the binary, set rpath/runpath correctly.
- [x] Document install/run layout and supported platforms.
- [x] Produce release artifacts (tar.gz) for macOS + Linux.
