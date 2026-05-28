// Minimal shell_state shim for the Rust CLI's linenoise build.
#pragma once

#include "duckdb/common/numeric_utils.hpp"
#include <string>

extern "C" int duckdb_shell_sqlite3_complete(const char *zSql);

namespace duckdb_shell {

struct ShellState {
	static bool SQLIsComplete(const char *zSql) {
		return duckdb_shell_sqlite3_complete(zSql) != 0;
	}

#if defined(_WIN32) || defined(WIN32)
	static std::wstring Win32Utf8ToUnicode(const std::string &zText) {
		return std::wstring(zText.begin(), zText.end());
	}
#endif
};

} // namespace duckdb_shell
