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
  mingw*|msys*|cygwin*) platform="windows" ; vendor_platform="windows" ;;
  *)
    echo "unsupported host OS: ${os}" >&2
    exit 2
    ;;
esac

lib_file=""
exe_ext=""
case "${platform}" in
  macos) lib_file="libduckdb.dylib" ;;
  linux) lib_file="libduckdb.so" ;;
  windows) lib_file="duckdb.dll" ; exe_ext=".exe" ;;
esac

lib_path=""
link_lib_dir=""
if [[ -n "${DUCKDB_LIB_DIR:-}" ]]; then
  link_lib_dir="${DUCKDB_LIB_DIR%/}"
  lib_path="${link_lib_dir}/${lib_file}"
else
  vendor_lib_dir="$PWD/vendor/duckdb/${version}/lib/${vendor_platform}"
  link_lib_dir="${vendor_lib_dir}"
  lib_path="${link_lib_dir}/${lib_file}"
fi

include_dir=""
if [[ -n "${DUCKDB_INCLUDE_DIR:-}" ]]; then
  include_dir="${DUCKDB_INCLUDE_DIR%/}"
else
  include_dir="$PWD/vendor/duckdb/${version}/include"
fi

if [[ ! -f "${lib_path}" ]]; then
  echo "missing libduckdb: ${lib_path}" >&2
  echo "set DUCKDB_LIB_DIR to the directory containing ${lib_file}" >&2
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

if [[ "${platform}" == "windows" ]]; then
  cp -f "${CARGO_TARGET_DIR}/release/duckdb_cli${exe_ext}" "${out_dir}/${pkg_name}/duckdb${exe_ext}"
else
  cp -f "${CARGO_TARGET_DIR}/release/duckdb_cli" "${out_dir}/${pkg_name}/duckdb"
fi
cp -f "${lib_path}" "${out_dir}/${pkg_name}/${lib_file}"
cp -R "${include_dir}" "${out_dir}/${pkg_name}/include"

pkg_binary="${out_dir}/${pkg_name}/duckdb${exe_ext}"
if [[ "${platform}" == "macos" ]] && command -v install_name_tool >/dev/null 2>&1; then
  for rpath in "${link_lib_dir}" "$PWD/build/release/src" "$PWD/build/debug/src"; do
    if [[ -n "${rpath}" ]]; then
      install_name_tool -delete_rpath "${rpath}" "${pkg_binary}" >/dev/null 2>&1 || true
    fi
  done
  if command -v otool >/dev/null 2>&1; then
    while IFS= read -r rpath; do
      case "${rpath}" in
        "${PWD}"*|*"/vendor/duckdb/"*|*"/build/release/src"|*"/build/debug/src")
          install_name_tool -delete_rpath "${rpath}" "${pkg_binary}" >/dev/null 2>&1 || true
          ;;
      esac
    done < <(otool -l "${pkg_binary}" | awk '/path / {print $2}')
  fi
fi

if [[ "${platform}" == "linux" ]]; then
  if ! command -v patchelf >/dev/null 2>&1; then
    echo "patchelf is required to normalize Linux package rpaths" >&2
    exit 2
  fi
  patchelf --set-rpath '$ORIGIN:$ORIGIN/../lib' "${pkg_binary}"
fi

if [[ "${platform}" == "macos" && -n "${CODESIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "${CODESIGN_IDENTITY}" "${pkg_binary}"
elif [[ "${platform}" == "macos" && "${MACOS_AD_HOC_SIGN:-0}" == "1" ]]; then
  codesign --force --sign - "${pkg_binary}" >/dev/null 2>&1 || true
fi

archive="${out_dir}/${pkg_name}.tar.gz"
tar -C "${out_dir}" -czf "${archive}" "${pkg_name}"
if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "${archive}" > "${archive}.sha256"
elif command -v sha256sum >/dev/null 2>&1; then
  sha256sum "${archive}" > "${archive}.sha256"
fi
echo "${archive}"
