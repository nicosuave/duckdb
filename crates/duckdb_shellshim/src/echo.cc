#include <cstddef>
#include <cstdint>
#include <cstring>
#include <cstdlib>
#include <memory>
#include <string>
#include <vector>

#include "duckdb.h"

#define DUCKDB_SHELLSHIM_STRINGIFY_IMPL(x) #x
#define DUCKDB_SHELLSHIM_STRINGIFY(x) DUCKDB_SHELLSHIM_STRINGIFY_IMPL(x)

struct DuckDBShellShimSQLStatementPrefix {
	void *vptr;
	uint8_t type;
	uint8_t pad[7];
	uint64_t stmt_location;
	uint64_t stmt_length;
};

struct DuckDBShellShimExtractStatementsWrapper {
	// Matches duckdb::ExtractStatementsWrapper layout (vector<unique_ptr<SQLStatement>> + string error).
	struct SQLStatement;
	std::vector<std::unique_ptr<SQLStatement>> statements;
	std::string error;
};

extern "C" {

const char *duckdb_shellshim_compiler_version() {
#if defined(__clang__) && defined(__clang_major__)
	return "clang-" DUCKDB_SHELLSHIM_STRINGIFY(__clang_major__) "." DUCKDB_SHELLSHIM_STRINGIFY(
	    __clang_minor__) "." DUCKDB_SHELLSHIM_STRINGIFY(__clang_patchlevel__);
#elif defined(__GNUC__) && defined(__GNUC_PATCHLEVEL__)
	return "gcc-" DUCKDB_SHELLSHIM_STRINGIFY(__GNUC__) "." DUCKDB_SHELLSHIM_STRINGIFY(
	    __GNUC_MINOR__) "." DUCKDB_SHELLSHIM_STRINGIFY(__GNUC_PATCHLEVEL__);
#else
	return "";
#endif
}

static bool duckdb_shellshim_is_space(char c) {
	return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}

int duckdb_shellshim_echo_slices_from_extracted(duckdb_extracted_statements extracted_statements, const char *query,
                                                char ***out_slices, size_t *out_count, char **out_error) {
	if (!extracted_statements || !query || !out_slices || !out_count) {
		if (out_error) {
			*out_error = strdup("duckdb_shellshim_echo_slices_from_extracted: invalid arguments");
		}
		return 1;
	}
	*out_slices = nullptr;
	*out_count = 0;
	if (out_error) {
		*out_error = nullptr;
	}

	auto *wrapper = reinterpret_cast<DuckDBShellShimExtractStatementsWrapper *>(extracted_statements);
	const auto count = wrapper->statements.size();

	auto *arr = static_cast<char **>(malloc(sizeof(char *) * count));
	if (!arr && count > 0) {
		if (out_error) {
			*out_error = strdup("duckdb_shellshim_echo_slices_from_extracted: malloc failed");
		}
		return 1;
	}
	for (size_t i = 0; i < count; i++) {
		arr[i] = nullptr;
	}

	const std::string sql(query);
	for (size_t i = 0; i < count; i++) {
		auto *stmt = wrapper->statements[i].get();
		auto *prefix = reinterpret_cast<const DuckDBShellShimSQLStatementPrefix *>(stmt);
		auto start_pos = static_cast<size_t>(prefix->stmt_location);
		auto len = static_cast<size_t>(prefix->stmt_length);

		while (len > 0 && start_pos < sql.size() && duckdb_shellshim_is_space(sql[start_pos])) {
			start_pos++;
			len--;
		}

		std::string slice;
		if (len > 0 && start_pos < sql.size()) {
			slice = sql.substr(start_pos, len);
		}
		if (slice.empty()) {
			slice = sql;
		}

		arr[i] = strdup(slice.c_str());
		if (!arr[i]) {
			for (size_t j = 0; j < count; j++) {
				free(arr[j]);
			}
			free(arr);
			if (out_error) {
				*out_error = strdup("duckdb_shellshim_echo_slices_from_extracted: strdup failed");
			}
			return 1;
		}
	}

	*out_slices = arr;
	*out_count = count;
	return 0;
}

void duckdb_shellshim_free_echo_slices(char **slices, size_t count) {
	if (!slices) {
		return;
	}
	for (size_t i = 0; i < count; i++) {
		free(slices[i]);
	}
	free(slices);
}

} // extern "C"
