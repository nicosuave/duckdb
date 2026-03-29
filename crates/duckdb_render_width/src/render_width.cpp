#include <cstddef>
#include <cstdint>
#include <algorithm>

#include "utf8proc.hpp"

static bool handle_ansi_escape(const char *buf, size_t len, size_t &cpos) {
	if (buf[cpos] != '\033') {
		return false;
	}
	cpos++;
	if (cpos < len && buf[cpos] == '[') {
		// CSI sequence
		cpos++;
		while (cpos < len && !(buf[cpos] >= '@' && buf[cpos] <= '~')) {
			cpos++;
		}
		if (cpos < len) {
			cpos++;
		}
	} else {
		// standalone ESC
		cpos++;
	}
	return true;
}

static bool utf8_iterate_at(const char *buf, size_t len, size_t pos, utf8proc_int32_t &codepoint, int &sz) {
	auto rc = duckdb::utf8proc_iterate(reinterpret_cast<const utf8proc_uint8_t *>(buf + pos),
	                                   (utf8proc_ssize_t)(len - pos), &codepoint);
	if (rc < 0) {
		return false;
	}
	sz = (int)rc;
	return true;
}

static size_t next_grapheme_cluster(const char *buf, size_t len, size_t pos) {
	utf8proc_int32_t prev_codepoint;
	int sz;
	if (!utf8_iterate_at(buf, len, pos, prev_codepoint, sz)) {
		return std::min(pos + 1, len);
	}

	utf8proc_int32_t state = 0;
	size_t cpos = pos + (size_t)sz;
	while (cpos < len) {
		utf8proc_int32_t next_codepoint;
		int next_sz;
		if (!utf8_iterate_at(buf, len, cpos, next_codepoint, next_sz)) {
			return std::min(cpos + 1, len);
		}
		if (duckdb::utf8proc_grapheme_break_stateful(prev_codepoint, next_codepoint, &state)) {
			return cpos;
		}
		prev_codepoint = next_codepoint;
		cpos += (size_t)next_sz;
	}
	return cpos;
}

static bool is_valid_utf8(const char *buf, size_t len) {
	size_t pos = 0;
	while (pos < len) {
		utf8proc_int32_t cp;
		auto rc = duckdb::utf8proc_iterate(reinterpret_cast<const utf8proc_uint8_t *>(buf + pos),
		                                   (utf8proc_ssize_t)(len - pos), &cp);
		if (rc < 0) {
			return false;
		}
		pos += (size_t)rc;
	}
	return true;
}

extern "C" {

size_t duckdb_cli_compute_render_width(const char *buf, size_t len) {
	size_t cpos = 0;
	size_t render_width = 0;
	while (cpos < len) {
		if (buf[cpos] == '\n') {
			render_width = 0;
			cpos++;
			continue;
		}
		if (buf[cpos] == '\t') {
			render_width += 8 - (render_width % 8);
			cpos++;
			continue;
		}
		if (handle_ansi_escape(buf, len, cpos)) {
			continue;
		}

		utf8proc_int32_t codepoint;
		int sz;
		if (!utf8_iterate_at(buf, len, cpos, codepoint, sz)) {
			cpos++;
			render_width++;
			continue;
		}
		size_t cluster_end = next_grapheme_cluster(buf, len, cpos);
		auto properties = duckdb::utf8proc_get_property(codepoint);
		render_width += (size_t)properties->charwidth;
		cpos = cluster_end;
	}
	return render_width;
}

int duckdb_cli_get_render_position(const char *buf, size_t len, int max_width, int *n) {
	if (!is_valid_utf8(buf, len)) {
		return -1;
	}
	size_t cpos = 0;
	size_t render_width = 0;
	while (cpos < len) {
		utf8proc_int32_t codepoint;
		int sz;
		if (!utf8_iterate_at(buf, len, cpos, codepoint, sz)) {
			return -1;
		}
		size_t cluster_end = next_grapheme_cluster(buf, len, cpos);
		auto properties = duckdb::utf8proc_get_property(codepoint);
		size_t char_render_width = (size_t)properties->charwidth;
		if ((int)(render_width + char_render_width) > max_width) {
			if (n) {
				*n = (int)render_width;
			}
			return (int)cpos;
		}
		cpos = cluster_end;
		render_width += char_render_width;
	}
	if (n) {
		*n = (int)render_width;
	}
	return (int)len;
}

size_t duckdb_cli_compute_render_width_duckbox(const char *buf, size_t len) {
	size_t cpos = 0;
	size_t render_width = 0;
	while (cpos < len) {
		if (buf[cpos] == '\n') {
			render_width = 0;
			cpos++;
			continue;
		}
		if (buf[cpos] == '\t') {
			render_width += 8 - (render_width % 8);
			cpos++;
			continue;
		}
		if (handle_ansi_escape(buf, len, cpos)) {
			continue;
		}

		utf8proc_int32_t codepoint;
		int sz;
		if (!utf8_iterate_at(buf, len, cpos, codepoint, sz)) {
			cpos++;
			render_width++;
			continue;
		}
		size_t cluster_end = next_grapheme_cluster(buf, len, cpos);
		size_t cluster_pos = cpos;
		size_t cluster_width = 0;
		while (cluster_pos < cluster_end) {
			utf8proc_int32_t cp;
			int cp_sz;
			if (!utf8_iterate_at(buf, len, cluster_pos, cp, cp_sz)) {
				cluster_pos++;
				cluster_width++;
				continue;
			}
			auto properties = duckdb::utf8proc_get_property(cp);
			cluster_width += (size_t)properties->charwidth;
			cluster_pos += (size_t)cp_sz;
		}
		render_width += cluster_width;
		cpos = cluster_end;
	}
	return render_width;
}

int duckdb_cli_get_render_position_duckbox(const char *buf, size_t len, int max_width, int *n) {
	if (!is_valid_utf8(buf, len)) {
		return -1;
	}
	size_t cpos = 0;
	size_t render_width = 0;
	while (cpos < len) {
		utf8proc_int32_t codepoint;
		int sz;
		if (!utf8_iterate_at(buf, len, cpos, codepoint, sz)) {
			return -1;
		}
		size_t cluster_end = next_grapheme_cluster(buf, len, cpos);
		size_t cluster_pos = cpos;
		size_t char_render_width = 0;
		while (cluster_pos < cluster_end) {
			utf8proc_int32_t cp;
			int cp_sz;
			if (!utf8_iterate_at(buf, len, cluster_pos, cp, cp_sz)) {
				cluster_pos++;
				char_render_width++;
				continue;
			}
			auto properties = duckdb::utf8proc_get_property(cp);
			char_render_width += (size_t)properties->charwidth;
			cluster_pos += (size_t)cp_sz;
		}
		if ((int)(render_width + char_render_width) > max_width) {
			if (n) {
				*n = (int)render_width;
			}
			return (int)cpos;
		}
		cpos = cluster_end;
		render_width += char_render_width;
	}
	if (n) {
		*n = (int)render_width;
	}
	return (int)len;
}

} // extern "C"
