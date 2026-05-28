// Minimal shell_highlight shim for the Rust CLI's linenoise build.
#pragma once

#include "duckdb/common/common.hpp"

namespace duckdb_shell {

enum class PrintIntensity { STANDARD, BOLD, UNDERLINE, BOLD_UNDERLINE };

enum class PrintColor : uint16_t {
	STANDARD = 256,
	BLACK = 0,
	RED = 1,
	GREEN = 2,
	YELLOW = 3,
	BLUE = 4,
	MAGENTA = 5,
	CYAN = 6,
	BRIGHTGRAY = 7,
	GRAY = 8,
	BRIGHTRED = 9,
	BRIGHTGREEN = 10,
	BRIGHTYELLOW = 11,
	BRIGHTBLUE = 12,
	BRIGHTMAGENTA = 13,
	BRIGHTCYAN = 14,
	WHITE = 15,
	ORANGE3 = 172,
	DEEPSKYBLUE1 = 39,
	DARKORANGE = 208,
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
		auto elements = Elements();
		auto index = static_cast<uint32_t>(type);
		auto max_index = static_cast<uint32_t>(HighlightElementType::NONE);
		if (index > max_index) {
			index = max_index;
		}
		return elements[index];
	}

	static bool SetHighlightElement(const char *name, PrintColor color, PrintIntensity intensity,
	                                bool user_configured = true) {
		if (!name) {
			return false;
		}
		auto max_index = static_cast<uint32_t>(HighlightElementType::NONE);
		for (uint32_t i = 0; i <= max_index; i++) {
			if (ElementNameEquals(Elements()[i].name, name)) {
				SetHighlightElement(static_cast<HighlightElementType>(i), color, intensity, user_configured);
				return true;
			}
		}
		return false;
	}

	static void SetHighlightElement(HighlightElementType type, PrintColor color, PrintIntensity intensity,
	                                bool user_configured = true) {
		auto index = static_cast<uint32_t>(type);
		auto max_index = static_cast<uint32_t>(HighlightElementType::NONE);
		if (index > max_index) {
			index = max_index;
		}
		if (user_configured) {
			UserConfiguredElements()[index] = true;
		} else if (UserConfiguredElements()[index]) {
			return;
		}
		Elements()[index].color = color;
		Elements()[index].intensity = intensity;
	}

	static void ToggleMode(int mode) {
		auto user_configured = false;
		if (mode == 1) {
			SetHighlightElement(HighlightElementType::KEYWORD, PrintColor::GREEN, PrintIntensity::STANDARD,
			                    user_configured);
			SetHighlightElement(HighlightElementType::STRING_CONSTANT, PrintColor::YELLOW, PrintIntensity::STANDARD,
			                    user_configured);
			SetHighlightElement(HighlightElementType::NUMERIC_CONSTANT, PrintColor::YELLOW, PrintIntensity::STANDARD,
			                    user_configured);
			SetHighlightElement(HighlightElementType::CONTINUATION_SELECTED, PrintColor::GREEN,
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::PROMPT, PrintColor::DARKORANGE, PrintIntensity::BOLD,
			                    user_configured);
			SetHighlightElement(HighlightElementType::DATABASE_NAME, PrintColor::ORANGE3, PrintIntensity::STANDARD,
			                    user_configured);
			SetHighlightElement(HighlightElementType::SCHEMA_NAME, PrintColor::DEEPSKYBLUE1,
			                    PrintIntensity::STANDARD, user_configured);
		} else if (mode == 2) {
			SetHighlightElement(HighlightElementType::KEYWORD, static_cast<PrintColor>(33), PrintIntensity::BOLD,
			                    user_configured);
			SetHighlightElement(HighlightElementType::STRING_CONSTANT, static_cast<PrintColor>(220),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::NUMERIC_CONSTANT, static_cast<PrintColor>(212),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::CONTINUATION_SELECTED, static_cast<PrintColor>(33),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::ERROR_TOKEN, static_cast<PrintColor>(203),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::ERROR_EMPHASIS, static_cast<PrintColor>(203),
			                    PrintIntensity::BOLD, user_configured);
			SetHighlightElement(HighlightElementType::ERROR_SUGGESTION, static_cast<PrintColor>(203),
			                    PrintIntensity::BOLD, user_configured);
		} else if (mode == 3) {
			SetHighlightElement(HighlightElementType::KEYWORD, static_cast<PrintColor>(27), PrintIntensity::BOLD,
			                    user_configured);
			SetHighlightElement(HighlightElementType::STRING_CONSTANT, static_cast<PrintColor>(58),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::NUMERIC_CONSTANT, static_cast<PrintColor>(90),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::CONTINUATION_SELECTED, static_cast<PrintColor>(27),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::PROMPT, static_cast<PrintColor>(166), PrintIntensity::BOLD,
			                    user_configured);
			SetHighlightElement(HighlightElementType::ERROR_TOKEN, static_cast<PrintColor>(124),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::ERROR_EMPHASIS, static_cast<PrintColor>(124),
			                    PrintIntensity::BOLD, user_configured);
			SetHighlightElement(HighlightElementType::ERROR_SUGGESTION, static_cast<PrintColor>(124),
			                    PrintIntensity::BOLD, user_configured);
			SetHighlightElement(HighlightElementType::DATABASE_NAME, static_cast<PrintColor>(166),
			                    PrintIntensity::STANDARD, user_configured);
			SetHighlightElement(HighlightElementType::SCHEMA_NAME, static_cast<PrintColor>(25),
			                    PrintIntensity::STANDARD, user_configured);
		}
	}

	static HighlightElement *Elements() {
		static HighlightElement elements[] = {
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
		    {"footer", PrintColor::STANDARD, PrintIntensity::STANDARD},
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
		    {"prompt", PrintColor::DARKORANGE, PrintIntensity::BOLD},
		    {"error_emphasis", PrintColor::RED, PrintIntensity::BOLD},
		    {"error_suggestion", PrintColor::RED, PrintIntensity::BOLD},
		    {"log_trace", PrintColor::BLUE, PrintIntensity::BOLD},
		    {"log_debug", PrintColor::YELLOW, PrintIntensity::BOLD},
		    {"log_info", PrintColor::GREEN, PrintIntensity::BOLD},
		    {"log_warning", PrintColor::ORANGE3, PrintIntensity::BOLD},
		    {"none", PrintColor::STANDARD, PrintIntensity::STANDARD},
		};
		return elements;
	}

	static bool *UserConfiguredElements() {
		static bool user_configured[static_cast<uint32_t>(HighlightElementType::NONE) + 1] = {};
		return user_configured;
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
	static char LowerAscii(char c) {
		if (c >= 'A' && c <= 'Z') {
			return c + ('a' - 'A');
		}
		return c;
	}

	static bool ElementNameEquals(const char *left, const char *right) {
		if (!left || !right) {
			return false;
		}
		while (*left && *right) {
			if (LowerAscii(*left) != LowerAscii(*right)) {
				return false;
			}
			left++;
			right++;
		}
		return *left == *right;
	}

	inline static bool highlighting_enabled = true;
};

} // namespace duckdb_shell
