#include "linenoise.hpp"

extern "C" void linenoiseSetErrorRendering(int enabled) {
	if (enabled) {
		duckdb::Linenoise::EnableErrorRendering();
	} else {
		duckdb::Linenoise::DisableErrorRendering();
	}
}

extern "C" void linenoiseSetCompletionRendering(int enabled) {
	if (enabled) {
		duckdb::Linenoise::EnableCompletionRendering();
	} else {
		duckdb::Linenoise::DisableCompletionRendering();
	}
}

