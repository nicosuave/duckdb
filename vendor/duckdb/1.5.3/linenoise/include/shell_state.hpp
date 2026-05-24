// Minimal shell_state shim for the Rust CLI's vendored linenoise build.
#pragma once

extern "C" int duckdb_shell_sqlite3_complete(const char *sql);

namespace duckdb_shell {

struct ShellState {
	static bool SQLIsComplete(const char *sql) {
		return duckdb_shell_sqlite3_complete(sql) != 0;
	}
};

} // namespace duckdb_shell
