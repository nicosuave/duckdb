#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

version="${DUCKDB_VENDOR_VERSION:-1.5.3}"
out_dir="${OUT_DIR:-$PWD/rust_cli/dist}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"

os="$(uname -s | tr '[:upper:]' '[:lower:]')"
arch="$(uname -m)"

case "${os}" in
  darwin) platform="macos" ; vendor_platform="darwin" ;;
  linux) platform="linux" ; vendor_platform="linux" ;;
  *)
    echo "unsupported host OS: ${os}" >&2
    exit 2
    ;;
esac

lib_ext=""
case "${platform}" in
  macos) lib_ext="dylib" ;;
  linux) lib_ext="so" ;;
esac

lib_path=""
if [[ -n "${DUCKDB_LIB_DIR:-}" ]]; then
  lib_path="${DUCKDB_LIB_DIR%/}/libduckdb.${lib_ext}"
else
  vendor_lib_dir="$PWD/vendor/duckdb/${version}/lib/${vendor_platform}"
  lib_path="${vendor_lib_dir}/libduckdb.${lib_ext}"
fi

include_dir=""
if [[ -n "${DUCKDB_INCLUDE_DIR:-}" ]]; then
  include_dir="${DUCKDB_INCLUDE_DIR%/}"
else
  include_dir="$PWD/vendor/duckdb/${version}/include"
fi

if [[ ! -f "${lib_path}" ]]; then
  echo "missing libduckdb: ${lib_path}" >&2
  echo "set DUCKDB_LIB_DIR to the directory containing libduckdb.${lib_ext}" >&2
  exit 2
fi
if [[ ! -d "${include_dir}" ]]; then
  echo "missing DuckDB include dir: ${include_dir}" >&2
  echo "set DUCKDB_INCLUDE_DIR to the DuckDB include directory" >&2
  exit 2
fi

pkg_name="duckdb-rust-cli-${version}-${platform}-${arch}"
stage_dir="$(mktemp -d)"
trap 'rm -rf "${stage_dir}"' EXIT

rm -rf "${out_dir}/${pkg_name}"
mkdir -p "${out_dir}/${pkg_name}"

cargo build -p duckdb_cli --release

cp -f "${CARGO_TARGET_DIR}/release/duckdb_cli" "${out_dir}/${pkg_name}/duckdb"
cp -f "${lib_path}" "${out_dir}/${pkg_name}/libduckdb.${lib_ext}"
cp -R "${include_dir}" "${out_dir}/${pkg_name}/include"

tar -C "${out_dir}" -czf "${out_dir}/${pkg_name}.tar.gz" "${pkg_name}"
echo "${out_dir}/${pkg_name}.tar.gz"
