#include "linenoise.hpp"
#include "linenoise.h"
#include "highlighting.hpp"
#include <cctype>
#include <cstring>
#include <sstream>
#include <string_view>

#if defined(_WIN32) || defined(__WIN32__) || defined(WIN32)
// disable highlighting on windows (for now?)
#define DISABLE_HIGHLIGHT
#endif

namespace duckdb {

#ifdef DISABLE_HIGHLIGHT
static int enableHighlighting = 0;
#else
static int enableHighlighting = 1;
#endif
struct Color {
	const char *color_name;
	const char *highlight;
};
static Color terminal_colors[] = {{"red", "\033[31m"},           {"green", "\033[32m"},
                                  {"yellow", "\033[33m"},        {"blue", "\033[34m"},
                                  {"magenta", "\033[35m"},       {"cyan", "\033[36m"},
                                  {"white", "\033[37m"},         {"brightblack", "\033[90m"},
                                  {"brightred", "\033[91m"},     {"brightgreen", "\033[92m"},
                                  {"brightyellow", "\033[93m"},  {"brightblue", "\033[94m"},
                                  {"brightmagenta", "\033[95m"}, {"brightcyan", "\033[96m"},
                                  {"brightwhite", "\033[97m"},   {nullptr, nullptr}};
static std::string bold = "\033[1m";
static std::string underline = "\033[4m";
static std::string keyword = "\033[32m";
static std::string continuation_selected = "\033[32m";
static std::string constant = "\033[33m";
static std::string continuation = "\033[90m";
static std::string comment = "\033[90m";
static std::string error = "\033[31m";
static std::string reset = "\033[00m";

void Highlighting::Enable() {
	enableHighlighting = 1;
}

void Highlighting::Disable() {
	enableHighlighting = 0;
}

bool Highlighting::IsEnabled() {
	return enableHighlighting;
}

const char *Highlighting::GetColorOption(const char *option) {
	size_t index = 0;
	while (terminal_colors[index].color_name) {
		if (strcmp(terminal_colors[index].color_name, option) == 0) {
			return terminal_colors[index].highlight;
		}
		index++;
	}
	return nullptr;
}

void Highlighting::SetHighlightingColor(HighlightingType type, const char *color) {
	switch (type) {
	case HighlightingType::KEYWORD:
		keyword = color;
		break;
	case HighlightingType::CONSTANT:
		constant = color;
		break;
	case HighlightingType::COMMENT:
		comment = color;
		break;
	case HighlightingType::ERROR:
		error = color;
		break;
	case HighlightingType::CONTINUATION:
		continuation = color;
		break;
	case HighlightingType::CONTINUATION_SELECTED:
		continuation_selected = color;
		break;
	}
}

static inline bool ascii_is_space(unsigned char c) {
	return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}

static inline bool ascii_is_digit(unsigned char c) {
	return c >= '0' && c <= '9';
}

static inline bool ascii_is_alpha(unsigned char c) {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
}

static inline bool ascii_is_ident_start(unsigned char c) {
	return ascii_is_alpha(c) || c == '_';
}

static inline bool ascii_is_ident_continue(unsigned char c) {
	return ascii_is_ident_start(c) || ascii_is_digit(c) || c == '$';
}

static inline unsigned char ascii_upper(unsigned char c) {
	if (c >= 'a' && c <= 'z') {
		return static_cast<unsigned char>(c - 'a' + 'A');
	}
	return c;
}

static bool ascii_iequals(std::string_view a, std::string_view b) {
	if (a.size() != b.size()) {
		return false;
	}
	for (size_t i = 0; i < a.size(); i++) {
		auto ca = ascii_upper(static_cast<unsigned char>(a[i]));
		auto cb = ascii_upper(static_cast<unsigned char>(b[i]));
		if (ca != cb) {
			return false;
		}
	}
	return true;
}

static bool is_keyword(std::string_view token) {
	static constexpr std::string_view keywords[] = {
	    "as",       "between", "by",       "case",     "cast",      "create",  "delete",   "distinct",
	    "drop",     "else",    "end",      "except",   "exists",    "from",    "full",     "group",
	    "having",   "insert",  "intersect","into",     "is",        "join",    "left",     "like",
	    "limit",    "not",     "null",     "offset",   "on",        "or",      "order",    "outer",
	    "over",     "replace", "right",    "select",   "set",       "table",   "then",     "union",
	    "update",   "values",  "when",     "where",    "with",
	};
	for (auto kw : keywords) {
		if (ascii_iequals(token, kw)) {
			return true;
		}
	}
	return false;
}

static vector<highlightToken> GetSqlTokens(char *buf, size_t len) {
	vector<highlightToken> tokens;
	if (len == 0) {
		return tokens;
	}
	highlightToken base_token;
	base_token.type = tokenType::TOKEN_IDENTIFIER;
	base_token.start = 0;
	tokens.push_back(base_token);

	auto current_type = tokenType::TOKEN_IDENTIFIER;
	size_t i = 0;
	while (i < len) {
		auto c = static_cast<unsigned char>(buf[i]);

		tokenType next_type = tokenType::TOKEN_IDENTIFIER;
		size_t consumed = 1;

		if (ascii_is_space(c)) {
			next_type = tokenType::TOKEN_IDENTIFIER;
			while (i + consumed < len && ascii_is_space(static_cast<unsigned char>(buf[i + consumed]))) {
				consumed++;
			}
		} else if (c == '-' && i + 1 < len && buf[i + 1] == '-') {
			next_type = tokenType::TOKEN_COMMENT;
			consumed = 2;
			while (i + consumed < len && buf[i + consumed] != '\n') {
				consumed++;
			}
		} else if (c == '/' && i + 1 < len && buf[i + 1] == '*') {
			next_type = tokenType::TOKEN_COMMENT;
			consumed = 2;
			while (i + consumed + 1 < len && !(buf[i + consumed] == '*' && buf[i + consumed + 1] == '/')) {
				consumed++;
			}
			if (i + consumed + 1 < len) {
				consumed += 2;
			}
		} else if (c == '\'') {
			next_type = tokenType::TOKEN_STRING_CONSTANT;
			consumed = 1;
			while (i + consumed < len) {
				if (buf[i + consumed] == '\'') {
					if (i + consumed + 1 < len && buf[i + consumed + 1] == '\'') {
						consumed += 2;
						continue;
					}
					consumed++;
					break;
				}
				consumed++;
			}
		} else if (ascii_is_digit(c) || (c == '.' && i + 1 < len && ascii_is_digit(static_cast<unsigned char>(buf[i + 1])))) {
			next_type = tokenType::TOKEN_NUMERIC_CONSTANT;
			consumed = 1;
			while (i + consumed < len) {
				auto nc = static_cast<unsigned char>(buf[i + consumed]);
				if (ascii_is_digit(nc) || nc == '.' || nc == '_' || nc == 'e' || nc == 'E' || nc == '+' || nc == '-') {
					consumed++;
					continue;
				}
				break;
			}
		} else if (ascii_is_ident_start(c)) {
			size_t start = i;
			consumed = 1;
			while (start + consumed < len && ascii_is_ident_continue(static_cast<unsigned char>(buf[start + consumed]))) {
				consumed++;
			}
			auto token = std::string_view(buf + start, consumed);
			next_type = is_keyword(token) ? tokenType::TOKEN_KEYWORD : tokenType::TOKEN_IDENTIFIER;
		} else if (c == '(' || c == ')' || c == '[' || c == ']' || c == '{' || c == '}') {
			next_type = tokenType::TOKEN_BRACKET;
		} else {
			next_type = tokenType::TOKEN_OPERATOR;
		}

		if (next_type != current_type) {
			highlightToken t;
			t.type = next_type;
			t.start = i;
			tokens.push_back(t);
			current_type = next_type;
		}

		i += consumed;
	}

	return tokens;
}

static vector<highlightToken> GetDotCommandTokens(char *buf, size_t len) {
	vector<highlightToken> tokens;

	// identifier token for the dot command itself
	highlightToken dot_token;
	dot_token.type = tokenType::TOKEN_KEYWORD;
	dot_token.start = 0;
	tokens.push_back(dot_token);

	for (idx_t i = 0; i + 1 < len; i++) {
		if (Linenoise::IsSpace(buf[i])) {
			highlightToken argument_token;
			argument_token.type = tokenType::TOKEN_STRING_CONSTANT;
			argument_token.start = i + 1;
			tokens.push_back(argument_token);
		}
	}
	return tokens;
}

vector<highlightToken> Highlighting::Tokenize(char *buf, size_t len, bool is_dot_command, searchMatch *match) {
	vector<highlightToken> tokens;
	if (!is_dot_command) {
		// SQL query - simplified tokenization (standalone build)
		tokens = GetSqlTokens(buf, len);
	} else {
		// . command
		tokens = GetDotCommandTokens(buf, len);
	}
	if (match) {
		// we have a search match - insert it into the token list
		// we want to insert a search token with start = match_start, end = match_end
		// first figure out which token type we would have at match_end (if any)
		for (size_t i = 0; i + 1 < tokens.size(); i++) {
			if (tokens[i].start <= match->match_start && tokens[i + 1].start >= match->match_start) {
				// this token begins after the search position, insert the token here
				size_t token_position = i + 1;
				auto end_type = tokens[i].type;
				if (tokens[i].start == match->match_start) {
					// exact start: only set the search match
					tokens[i].search_match = true;
				} else {
					// non-exact start: add a new token
					highlightToken search_token;
					search_token.type = tokens[i].type;
					search_token.start = match->match_start;
					search_token.search_match = true;
					tokens.insert(tokens.begin() + token_position, search_token);
					token_position++;
				}

				// move forwards
				while (token_position < tokens.size() && tokens[token_position].start < match->match_end) {
					// this token is
					// mark this token as a search token
					end_type = tokens[token_position].type;
					tokens[token_position].search_match = true;
					token_position++;
				}
				if (token_position >= tokens.size() || tokens[token_position].start > match->match_end) {
					// insert the token that marks the end of the search
					highlightToken end_token;
					end_token.type = end_type;
					end_token.start = match->match_end;
					tokens.insert(tokens.begin() + token_position, end_token);
					token_position++;
				}
				break;
			}
		}
	}
	return tokens;
}

string Highlighting::HighlightText(char *buf, size_t len, size_t start_pos, size_t end_pos,
                                   const vector<highlightToken> &tokens) {
	std::stringstream ss;
	size_t prev_pos = 0;
	for (size_t i = 0; i < tokens.size(); i++) {
		size_t next = i + 1 < tokens.size() ? tokens[i + 1].start : len;
		if (next < start_pos) {
			// this token is not rendered at all
			continue;
		}

		auto &token = tokens[i];
		size_t start = token.start > start_pos ? token.start : start_pos;
		size_t end = next > end_pos ? end_pos : next;
		if (end <= start) {
			continue;
		}
		if (prev_pos > start) {
#ifdef DEBUG
			throw InternalException(string("ERROR - Rendering at position ") + std::to_string(start) +
			                        " after rendering at position " + std::to_string(prev_pos));
#endif
			Linenoise::Log("ERROR - Rendering at position %llu after rendering at position %llu\n", start, prev_pos);
			continue;
		}
		prev_pos = start;
		std::string text = std::string(buf + start, end - start);
		if (token.search_match) {
			ss << underline;
		}
		switch (token.type) {
		case tokenType::TOKEN_KEYWORD:
			ss << keyword << text << reset;
			break;
		case tokenType::TOKEN_NUMERIC_CONSTANT:
		case tokenType::TOKEN_STRING_CONSTANT:
			ss << constant << text << reset;
			break;
		case tokenType::TOKEN_CONTINUATION:
			ss << continuation << text << reset;
			break;
		case tokenType::TOKEN_CONTINUATION_SELECTED:
			ss << continuation_selected << text << reset;
			break;
		case tokenType::TOKEN_BRACKET:
			ss << underline << text << reset;
			break;
		case tokenType::TOKEN_ERROR:
			ss << error << text << reset;
			break;
		case tokenType::TOKEN_COMMENT:
			ss << comment << text << reset;
			break;
		default:
			ss << text;
			if (token.search_match) {
				ss << reset;
			}
		}
	}
	return ss.str();
}

} // namespace duckdb
