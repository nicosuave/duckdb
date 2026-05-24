// Minimal shell_highlight shim for the Rust CLI's vendored linenoise build.
#pragma once

#include "duckdb/common/common.hpp"

namespace duckdb_shell {

enum class PrintIntensity { STANDARD, BOLD, UNDERLINE, BOLD_UNDERLINE };

enum class PrintColor : uint16_t {
	STANDARD = 256,
	RED = 1,
	GREEN = 2,
	YELLOW = 3,
	BLUE = 4,
	GRAY = 8,
	ORANGE3 = 172,
	DEEPSKYBLUE1 = 39,
};

enum class HighlightElementType : uint32_t {
	ERROR_TOKEN = 0,
	KEYWORD,
	NUMERIC_CONSTANT,
	STRING_CONSTANT,
	LINE_INDICATOR,
	DATABASE_NAME,
	SCHEMA_NAME,
	TABLE_NAME,
	COLUMN_NAME,
	COLUMN_TYPE,
	NUMERIC_VALUE,
	STRING_VALUE,
	TEMPORAL_VALUE,
	NULL_VALUE,
	FOOTER,
	LAYOUT,
	STARTUP_TEXT,
	STARTUP_VERSION,
	CONTINUATION,
	CONTINUATION_SELECTED,
	BRACKET,
	COMMENT,
	SUGGESTION_CATALOG_NAME,
	SUGGESTION_SCHEMA_NAME,
	SUGGESTION_TABLE_NAME,
	SUGGESTION_COLUMN_NAME,
	SUGGESTION_FILE_NAME,
	SUGGESTION_DIRECTORY_NAME,
	SUGGESTION_FUNCTION_NAME,
	SUGGESTION_SETTING_NAME,
	TABLE_LAYOUT,
	VIEW_LAYOUT,
	PRIMARY_KEY_COLUMN,
	PROMPT,
	ERROR_EMPHASIS,
	ERROR_SUGGESTION,
	LOG_TRACE,
	LOG_DEBUG,
	LOG_INFO,
	LOG_WARNING,
	NONE
};

struct HighlightElement {
	const char *name;
	PrintColor color;
	PrintIntensity intensity;
};

struct ShellHighlight {
	static bool IsEnabled() {
		return highlighting_enabled;
	}

	static void SetHighlighting(bool enabled) {
		highlighting_enabled = enabled;
	}

	static const HighlightElement &GetHighlightElement(HighlightElementType type) {
		static const HighlightElement elements[] = {
		    {"error", PrintColor::RED, PrintIntensity::STANDARD},
		    {"keyword", PrintColor::GREEN, PrintIntensity::STANDARD},
		    {"numeric_constant", PrintColor::YELLOW, PrintIntensity::STANDARD},
		    {"string_constant", PrintColor::YELLOW, PrintIntensity::STANDARD},
		    {"line_indicator", PrintColor::STANDARD, PrintIntensity::BOLD},
		    {"database_name", PrintColor::ORANGE3, PrintIntensity::STANDARD},
		    {"schema_name", PrintColor::DEEPSKYBLUE1, PrintIntensity::STANDARD},
		    {"table_name", PrintColor::STANDARD, PrintIntensity::BOLD},
		    {"column_name", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"column_type", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"numeric_value", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"string_value", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"temporal_value", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"null_value", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"footer", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"layout", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"startup_text", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"startup_version", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"continuation", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"continuation_selected", PrintColor::GREEN, PrintIntensity::STANDARD},
		    {"bracket", PrintColor::STANDARD, PrintIntensity::UNDERLINE},
		    {"comment", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"suggestion_catalog_name", PrintColor::ORANGE3, PrintIntensity::STANDARD},
		    {"suggestion_schema_name", PrintColor::DEEPSKYBLUE1, PrintIntensity::STANDARD},
		    {"suggestion_table_name", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"suggestion_column_name", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"suggestion_file_name", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"suggestion_directory_name", PrintColor::STANDARD, PrintIntensity::BOLD},
		    {"suggestion_function_name", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"suggestion_setting_name", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"table_layout", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"view_layout", PrintColor::STANDARD, PrintIntensity::STANDARD},
		    {"primary_key_column", PrintColor::STANDARD, PrintIntensity::UNDERLINE},
		    {"prompt", PrintColor::ORANGE3, PrintIntensity::BOLD},
		    {"error_emphasis", PrintColor::RED, PrintIntensity::BOLD},
		    {"error_suggestion", PrintColor::RED, PrintIntensity::BOLD},
		    {"log_trace", PrintColor::BLUE, PrintIntensity::BOLD},
		    {"log_debug", PrintColor::YELLOW, PrintIntensity::BOLD},
		    {"log_info", PrintColor::GREEN, PrintIntensity::BOLD},
		    {"log_warning", PrintColor::GRAY, PrintIntensity::STANDARD},
		    {"none", PrintColor::STANDARD, PrintIntensity::STANDARD},
		};
		auto index = static_cast<uint32_t>(type);
		auto max_index = static_cast<uint32_t>(HighlightElementType::NONE);
		if (index > max_index) {
			index = max_index;
		}
		return elements[index];
	}

	static duckdb::string TerminalCode(PrintColor color, PrintIntensity intensity) {
		duckdb::string result;
		switch (intensity) {
		case PrintIntensity::BOLD:
			result += "\x1b[1m";
			break;
		case PrintIntensity::UNDERLINE:
			result += "\x1b[4m";
			break;
		case PrintIntensity::BOLD_UNDERLINE:
			result += "\x1b[1m\x1b[4m";
			break;
		default:
			break;
		}
		if (color == PrintColor::STANDARD) {
			return result;
		}
		auto code = static_cast<uint16_t>(color);
		if (code >= 1 && code <= 7) {
			result += "\x1b[" + std::to_string(31 + code - 1) + "m";
		} else if (code >= 8 && code <= 15) {
			result += "\x1b[" + std::to_string(90 + code - 8) + "m";
		} else {
			result += "\x1b[38;5;" + std::to_string(code) + "m";
		}
		return result;
	}

	static duckdb::string ResetTerminalCode() {
		return "\x1b[00m";
	}

private:
	inline static bool highlighting_enabled = true;
};

} // namespace duckdb_shell
