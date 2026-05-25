#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

sql_file="${1:-}"
if [[ -z "${sql_file}" ]]; then
  echo "usage: bash rust_cli/diff_shells.sh path/to/script.sql" >&2
  exit 2
fi
if [[ ! -f "${sql_file}" ]]; then
  echo "missing file: ${sql_file}" >&2
  exit 2
fi

ref_shell="${DUCKDB_REF_SHELL:-}"
rust_shell="${DUCKDB_RUST_SHELL:-./target/debug/duckdb_cli}"

if [[ -z "${ref_shell}" ]]; then
  if command -v duckdb >/dev/null 2>&1; then
    ref_shell="$(command -v duckdb)"
  else
    ref_shell="./build/release/duckdb"
  fi
fi

if [[ ! -x "${ref_shell}" ]]; then
  echo "missing ref shell: ${ref_shell}" >&2
  exit 2
fi
if [[ ! -x "${rust_shell}" ]]; then
  echo "missing rust shell: ${rust_shell} (run 'cargo build -p duckdb_cli')" >&2
  exit 2
fi

tmp_ref="$(mktemp)"
tmp_rust="$(mktemp)"
trap 'rm -f "${tmp_ref}" "${tmp_rust}"' EXIT

set +e
"${ref_shell}" --batch --init /dev/null <"${sql_file}" >"${tmp_ref}" 2>&1
ref_status=$?
DUCKDB_SKIP_LIB_VERSION_CHECK=1 "${rust_shell}" --batch --init /dev/null <"${sql_file}" >"${tmp_rust}" 2>&1
rust_status=$?
set -e

diff -u "${tmp_ref}" "${tmp_rust}"

if [[ "${ref_status}" -ne "${rust_status}" ]]; then
  echo "exit status mismatch: ref=${ref_status} rust=${rust_status}" >&2
  exit 1
fi
