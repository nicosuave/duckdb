#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

export UV_CACHE_DIR="${UV_CACHE_DIR:-$PWD/.uv-cache}"
mkdir -p "${UV_CACHE_DIR}"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
export DUCKDB_SKIP_LIB_VERSION_CHECK=1

cargo build -p duckdb_cli
./target/debug/duckdb_cli -version >/dev/null
exec uv run python -m pytest -q tools/shell/tests --shell-binary ./target/debug/duckdb_cli "$@"
