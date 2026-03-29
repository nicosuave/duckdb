#if defined(_WIN32) || defined(WIN32)
extern "C" int duckdb_cli_get_terminal_color_mode() {
	return 0;
}
#else
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/select.h>
#include <sys/time.h>
#include <termios.h>
#include <unistd.h>

namespace {

struct TerminalColor {
	uint8_t r;
	uint8_t g;
	uint8_t b;
};

static bool has_more_data(int fd, int timeout_usec) {
	fd_set readfds;
	FD_ZERO(&readfds);
	FD_SET(fd, &readfds);
	struct timeval tv;
	tv.tv_sec = 0;
	tv.tv_usec = timeout_usec;
	return select(fd + 1, &readfds, nullptr, nullptr, &tv) > 0 && FD_ISSET(fd, &readfds);
}

static bool parse_terminal_color(TerminalColor &color, const char *buf, size_t buflen) {
	// Expected format contains: rgb:RRRR/GGGG/BBBB (hex, 16-bit components).
	size_t offset = 0;
	for (; offset + 4 < buflen; offset++) {
		if (memcmp(buf + offset, (const void *)"rgb:", 4) == 0) {
			break;
		}
	}
	offset += 4;
	if (offset >= buflen) {
		return false;
	}
	uint8_t values[3];
	memset(values, 0, sizeof(values));

	for (size_t k = 0; k < 3; k++) {
		if (k > 0) {
			if (offset >= buflen || buf[offset] != '/') {
				return false;
			}
			offset++;
		}
		uint32_t value = 0;
		size_t end_pos = offset + 4;
		for (; offset < end_pos; offset++) {
			if (offset >= buflen) {
				return false;
			}
			char c = buf[offset];
			if (c == '/') {
				break;
			}
			uint32_t current_value;
			if (c >= 'A' && c <= 'F') {
				current_value = 10 + (c - 'A');
			} else if (c >= 'a' && c <= 'f') {
				current_value = 10 + (c - 'a');
			} else if (c >= '0' && c <= '9') {
				current_value = c - '0';
			} else {
				return false;
			}
			value = value * 16 + current_value;
		}
		values[k] = static_cast<uint8_t>(value >> 8);
	}

	color.r = values[0];
	color.g = values[1];
	color.b = values[2];
	return true;
}

static bool enable_raw_mode(int fd, struct termios &orig) {
	if (tcgetattr(fd, &orig) == -1) {
		return false;
	}
	struct termios raw = orig;

	raw.c_iflag &= ~(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
	raw.c_oflag &= ~(OPOST);
	raw.c_cflag |= (CS8);
	raw.c_lflag &= ~(ECHO | ICANON | IEXTEN);

	raw.c_cc[VMIN] = 0;
	raw.c_cc[VTIME] = 1;

	return tcsetattr(fd, TCSAFLUSH, &raw) != -1;
}

static void disable_raw_mode(int fd, const struct termios &orig) {
	(void)tcsetattr(fd, TCSAFLUSH, &orig);
}

static bool try_get_background_color(TerminalColor &color) {
	const int ifd = STDIN_FILENO;
	const int ofd = STDOUT_FILENO;

	struct termios orig;
	if (!enable_raw_mode(ifd, orig)) {
		return false;
	}

	bool success = false;
	// OSC 11 query background color.
	const char query[] = "\x1b]11;?\x07";
	if (write(ofd, query, sizeof(query) - 1) == (ssize_t)(sizeof(query) - 1)) {
		char buf[64];
		size_t i = 0;
		while (i < sizeof(buf) - 1) {
			if (!has_more_data(ifd, 10000)) {
				break;
			}
			if (read(ifd, buf + i, 1) != 1) {
				break;
			}
			if (buf[i] == '\a') {
				break;
			}
			if (i > 2 && buf[i - 1] == '\x1b' && buf[i] == '\\') {
				i--;
				break;
			}
			i++;
		}
		buf[i] = '\0';
		success = parse_terminal_color(color, buf, i);
	}

	disable_raw_mode(ifd, orig);
	return success;
}

} // namespace

extern "C" int duckdb_cli_get_terminal_color_mode() {
	if (!isatty(STDIN_FILENO) || !isatty(STDOUT_FILENO)) {
		return 0; // unknown
	}
	TerminalColor background;
	if (!try_get_background_color(background)) {
		return 0; // unknown
	}
	const double brightness =
	    0.2126 * (double)background.r + 0.7152 * (double)background.g + 0.0722 * (double)background.b;
	if (brightness <= 96.0) {
		return 1; // dark
	}
	if (brightness >= 160.0) {
		return 2; // light
	}
	return 3; // mixed
}
#endif
