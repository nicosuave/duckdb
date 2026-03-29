#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

export UV_CACHE_DIR="${UV_CACHE_DIR:-$PWD/.uv-cache}"
mkdir -p "${UV_CACHE_DIR}"

export DUCKDB_LIB_SOURCE="${DUCKDB_LIB_SOURCE:-repo}"
export DUCKDB_SKIP_LIB_VERSION_CHECK=1

cargo build -p duckdb_cli
exec uv run python -m pytest -q tools/shell/tests --shell-binary ./target/debug/duckdb_cli "$@"
