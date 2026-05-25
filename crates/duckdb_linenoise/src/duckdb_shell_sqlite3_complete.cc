#include <cstddef>
#include <cstdint>
#include <cstring>

extern "C" {

enum class SQLParseState { SEMICOLON, WHITESPACE, NORMAL };

static const char *skipDollarQuotedString(const char *zSql, const char *delimiterStart, size_t delimiterLength) {
	for (; *zSql; zSql++) {
		if (*zSql == '$') {
			zSql++;
			auto start = zSql;
			while (*zSql && *zSql != '$') {
				zSql++;
			}
			if (!zSql[0]) {
				return nullptr;
			}
			if (delimiterLength == size_t(zSql - start)) {
				if (memcmp(start, delimiterStart, delimiterLength) == 0) {
					return zSql;
				}
			}
			zSql = start - 1;
		}
	}
	return nullptr;
}

int duckdb_shell_sqlite3_complete(const char *zSql) {
	if (!zSql) {
		return 0;
	}
	const char *trimmed = zSql;
	while (*trimmed == ' ' || *trimmed == '\r' || *trimmed == '\t' || *trimmed == '\n' || *trimmed == '\f') {
		trimmed++;
	}
	if (trimmed[0] == '\\' && trimmed[1] == 'e') {
		const char *tail = trimmed + 2;
		while (*tail == ' ' || *tail == '\r' || *tail == '\t' || *tail == '\n' || *tail == '\f') {
			tail++;
		}
		if (*tail == '\0') {
			return 1;
		}
	}
	auto state = SQLParseState::NORMAL;
	for (; *zSql; zSql++) {
		SQLParseState next_state;
		switch (*zSql) {
		case ';':
			next_state = SQLParseState::SEMICOLON;
			break;
		case ' ':
		case '\r':
		case '\t':
		case '\n':
		case '\f':
			next_state = SQLParseState::WHITESPACE;
			break;
		case '/': {
			if (zSql[1] != '*') {
				next_state = SQLParseState::NORMAL;
				break;
			}
			zSql += 2;
			while (zSql[0] && (zSql[0] != '*' || zSql[1] != '/')) {
				zSql++;
			}
			if (zSql[0] == 0) {
				return 0;
			}
			zSql++;
			next_state = SQLParseState::WHITESPACE;
			break;
		}
		case '-': {
			if (zSql[1] != '-') {
				next_state = SQLParseState::NORMAL;
				break;
			}
			while (*zSql && *zSql != '\n') {
				zSql++;
			}
			if (*zSql == 0) {
				return state == SQLParseState::SEMICOLON ? 1 : 0;
			}
			next_state = SQLParseState::WHITESPACE;
			break;
		}
		case '$': {
			size_t next_dollar = 0;
			for (size_t idx = 1; zSql[idx]; idx++) {
				if (zSql[idx] == '$') {
					next_dollar = idx;
					break;
				}
				auto ch = static_cast<unsigned char>(zSql[idx]);
				if (ch >= 'A' && ch <= 'Z') {
					continue;
				}
				if (ch >= 'a' && ch <= 'z') {
					continue;
				}
				if (ch >= 0x80 && ch <= 0xFF) {
					continue;
				}
				if (idx > 1 && ch >= '0' && ch <= '9') {
					continue;
				}
				break;
			}
			if (next_dollar == 0) {
				next_state = SQLParseState::NORMAL;
				break;
			}
			auto start = zSql + 1;
			zSql += next_dollar;
			const char *delimiterStart = start;
			size_t delimiterLength = size_t(zSql - start);
			zSql++;
			zSql = skipDollarQuotedString(zSql, delimiterStart, delimiterLength);
			if (!zSql) {
				return 0;
			}
			next_state = SQLParseState::WHITESPACE;
			break;
		}
		case '"':
		case '\'': {
			int c = *zSql;
			zSql++;
			while (*zSql && *zSql != c) {
				zSql++;
			}
			if (*zSql == 0) {
				return 0;
			}
			next_state = SQLParseState::WHITESPACE;
			break;
		}
		default:
			next_state = SQLParseState::NORMAL;
			break;
		}
		if (next_state != SQLParseState::WHITESPACE) {
			state = next_state;
		}
	}
	return state == SQLParseState::SEMICOLON ? 1 : 0;
}

} // extern "C"
