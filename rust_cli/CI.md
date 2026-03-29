# Rust CLI CI notes (macOS/Linux)

Goal: keep the Rust CLI compatible with `tools/shell/tests/` and make it easy to build a distributable binary.

## Local build

- Build the Rust CLI: `cargo build -p duckdb_cli`
- Run the shell test suite: `bash rust_cli/run_shell_tests.sh` (sets `UV_CACHE_DIR=$PWD/.uv-cache` for sandboxed runs)

## Packaging (macOS/Linux)

Build a tarball containing `duckdb` + a shipped `libduckdb` + headers:

- `bash rust_cli/package.sh`

Override defaults:

- `DUCKDB_VENDOR_VERSION=1.4.3 OUT_DIR=$PWD/rust_cli/dist bash rust_cli/package.sh`
- `DUCKDB_LIB_DIR=/path/to/libdir bash rust_cli/package.sh`

## Diff helper

To compare the Rust CLI against the repo-built shell on the same input script:

- `bash rust_cli/diff_shells.sh path/to/script.sql`

Override binaries if needed:

- `DUCKDB_REF_SHELL=... DUCKDB_RUST_SHELL=... bash rust_cli/diff_shells.sh ...`
