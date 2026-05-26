#include "shell_highlight.hpp"

#include <cstdint>

extern "C" void duckdb_cli_linenoise_set_highlighting(int enabled) {
	duckdb_shell::ShellHighlight::SetHighlighting(enabled != 0);
}

extern "C" int duckdb_cli_linenoise_set_highlight_color(const char *element, uint16_t color, int intensity,
                                                        int user_configured) {
	return duckdb_shell::ShellHighlight::SetHighlightElement(element, static_cast<duckdb_shell::PrintColor>(color),
	                                                         static_cast<duckdb_shell::PrintIntensity>(intensity),
	                                                         user_configured != 0)
	           ? 1
	           : 0;
}

extern "C" void duckdb_cli_linenoise_apply_highlight_mode(int mode) {
	duckdb_shell::ShellHighlight::ToggleMode(mode);
}
