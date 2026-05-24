use crate::state::PrintIntensity;

#[derive(Clone, Copy)]
struct HighlightColorInfo {
    name: &'static str,
    code: u8,
    r: u8,
    g: u8,
    b: u8,
}

fn hue(input: HighlightColorInfo) -> f64 {
    let r = (input.r as f64) / 255.0;
    let g = (input.g as f64) / 255.0;
    let b = (input.b as f64) / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta == 0.0 {
        return 0.0;
    }
    let mut hue = if max == r {
        (g - b) / delta
    } else if max == g {
        2.0 + (b - r) / delta
    } else {
        4.0 + (r - g) / delta
    };
    hue *= 60.0;
    if hue < 0.0 {
        hue += 360.0;
    }
    hue
}

fn lum(input: HighlightColorInfo) -> f64 {
    (0.241 * (input.r as f64) + 0.691 * (input.g as f64) + 0.068 * (input.b as f64)).sqrt()
}

fn find_color_group(name: &str) -> usize {
    const GROUPS: &[&[&str]] = &[
        &["red", "maroon", "coral"],
        &["orange"],
        &["yellow", "gold", "khaki", "wheat"],
        &["green", "chartreuse", "lime", "honeydew", "olive"],
        &["cyan", "aqua", "turquoise"],
        &["blue", "navy"],
        &["pink", "orchid", "rose", "thistle", "salmon", "tan"],
        &["purple", "magenta", "plum", "fuchsia", "violet"],
        &["brown"],
        &["grey", "gray", "black"],
        &["white", "silver", "cornsilk"],
    ];

    for (group_idx, group) in GROUPS.iter().enumerate() {
        for token in *group {
            if name.contains(token) {
                return group_idx;
            }
        }
    }
    // In DuckDB's shell this is an internal error, but for robustness fall back to the last group.
    GROUPS.len().saturating_sub(1)
}

fn intensity_prefix(intensity: PrintIntensity) -> &'static str {
    match intensity {
        PrintIntensity::Standard => "",
        PrintIntensity::Bold => "\x1b[1m",
        PrintIntensity::Underline => "\x1b[4m",
        PrintIntensity::BoldUnderline => "\x1b[1m\x1b[4m",
    }
}

fn color_prefix(code: u8) -> String {
    // Match `ShellHighlight::TerminalCode` in tools/shell/shell_highlight.cpp:
    // - standard colors RED..BRIGHTGRAY use 31..37 (black is NOT in this range)
    // - bright colors GRAY..WHITE use 90..97
    // - everything else (including BLACK=0) uses 38;5;{code}
    if (1..=7).contains(&code) {
        return format!("\x1b[{}m", 31u16 + (code as u16 - 1));
    }
    if (8..=15).contains(&code) {
        return format!("\x1b[{}m", 90u16 + (code as u16 - 8));
    }
    format!("\x1b[38;5;{}m", code)
}

pub fn render_display_colors(intensity: PrintIntensity, use_ansi: bool) -> String {
    let mut colors: Vec<HighlightColorInfo> = HIGHLIGHT_COLOR_INFO.to_vec();
    colors.sort_unstable_by(|a, b| {
        let a_group = find_color_group(a.name);
        let b_group = find_color_group(b.name);
        if a_group != b_group {
            return a_group.cmp(&b_group);
        }
        let a_hue = hue(*a);
        let b_hue = hue(*b);
        if a_hue != b_hue {
            return a_hue
                .partial_cmp(&b_hue)
                .unwrap_or(std::cmp::Ordering::Equal);
        }
        let a_lum = lum(*a);
        let b_lum = lum(*b);
        let lum_order = a_lum
            .partial_cmp(&b_lum)
            .unwrap_or(std::cmp::Ordering::Equal);
        if lum_order != std::cmp::Ordering::Equal {
            return lum_order;
        }
        b.code.cmp(&a.code)
    });

    let mut out = String::new();
    if use_ansi {
        let intensity = intensity_prefix(intensity);
        for c in colors {
            out.push_str(intensity);
            out.push_str(&color_prefix(c.code));
            out.push_str(c.name);
            out.push_str("\x1b[00m");
            out.push(' ');
        }
    } else {
        for c in colors {
            out.push_str(c.name);
            out.push(' ');
        }
    }
    out.push('\n');
    out
}

pub fn try_get_highlight_color_code(name: &str) -> Result<u8, String> {
    for c in HIGHLIGHT_COLOR_INFO {
        if c.name.eq_ignore_ascii_case(name) {
            return Ok(c.code);
        }
    }

    let mut error_msg = format!("Unknown highlighting color '{}'\n", name);
    let color_names: Vec<String> = HIGHLIGHT_COLOR_INFO
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    error_msg.push_str(&crate::candidates::candidates_error_message(
        &color_names,
        name,
        "Did you mean",
    ));
    error_msg.push('\n');
    error_msg.push_str("Run '.display_colors' for a list of available colors.\n");
    error_msg.push('\n');
    Err(error_msg)
}

// Port of DuckDB's `highlight_color_info[]` list (tools/shell/shell_highlight.cpp).
// Sorted at runtime to match `.display_colors` in tools/shell/shell_metadata_command.cpp.
static HIGHLIGHT_COLOR_INFO: &[HighlightColorInfo] = &[
    HighlightColorInfo {
        name: "black",
        code: 0,
        r: 0x00,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "red",
        code: 1,
        r: 0x80,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "green",
        code: 2,
        r: 0x00,
        g: 0x80,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "yellow",
        code: 3,
        r: 0x80,
        g: 0x80,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "blue",
        code: 4,
        r: 0x00,
        g: 0x00,
        b: 0x80,
    },
    HighlightColorInfo {
        name: "magenta",
        code: 5,
        r: 0x80,
        g: 0x00,
        b: 0x80,
    },
    HighlightColorInfo {
        name: "cyan",
        code: 6,
        r: 0x00,
        g: 0x80,
        b: 0x80,
    },
    HighlightColorInfo {
        name: "brightgray",
        code: 7,
        r: 0xC0,
        g: 0xC0,
        b: 0xC0,
    },
    HighlightColorInfo {
        name: "gray",
        code: 8,
        r: 0x80,
        g: 0x80,
        b: 0x80,
    },
    HighlightColorInfo {
        name: "brightred",
        code: 9,
        r: 0xFF,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "brightgreen",
        code: 10,
        r: 0x00,
        g: 0xFF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "brightyellow",
        code: 11,
        r: 0xFF,
        g: 0xFF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "brightblue",
        code: 12,
        r: 0x00,
        g: 0x00,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "brightmagenta",
        code: 13,
        r: 0xFF,
        g: 0x00,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "brightcyan",
        code: 14,
        r: 0x00,
        g: 0xFF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "white",
        code: 15,
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "grey0",
        code: 16,
        r: 0x00,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "navyblue",
        code: 17,
        r: 0x00,
        g: 0x00,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "darkblue",
        code: 18,
        r: 0x00,
        g: 0x00,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "blue3",
        code: 19,
        r: 0x00,
        g: 0x00,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "blue4",
        code: 20,
        r: 0x00,
        g: 0x00,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "blue1",
        code: 21,
        r: 0x00,
        g: 0x00,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "darkgreen",
        code: 22,
        r: 0x00,
        g: 0x5F,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "deepskyblue4",
        code: 23,
        r: 0x00,
        g: 0x5F,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "deepskyblue5",
        code: 24,
        r: 0x00,
        g: 0x5F,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "deepskyblue6",
        code: 25,
        r: 0x00,
        g: 0x5F,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "dodgerblue3",
        code: 26,
        r: 0x00,
        g: 0x5F,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "dodgerblue2",
        code: 27,
        r: 0x00,
        g: 0x5F,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "green4",
        code: 28,
        r: 0x00,
        g: 0x87,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "springgreen4",
        code: 29,
        r: 0x00,
        g: 0x87,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "turquoise4",
        code: 30,
        r: 0x00,
        g: 0x87,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "deepskyblue3",
        code: 31,
        r: 0x00,
        g: 0x87,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "deepskyblue7",
        code: 32,
        r: 0x00,
        g: 0x87,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "dodgerblue1",
        code: 33,
        r: 0x00,
        g: 0x87,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "green3",
        code: 34,
        r: 0x00,
        g: 0xAF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "springgreen3",
        code: 35,
        r: 0x00,
        g: 0xAF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "darkcyan",
        code: 36,
        r: 0x00,
        g: 0xAF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "lightseagreen",
        code: 37,
        r: 0x00,
        g: 0xAF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "deepskyblue2",
        code: 38,
        r: 0x00,
        g: 0xAF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "deepskyblue1",
        code: 39,
        r: 0x00,
        g: 0xAF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "green5",
        code: 40,
        r: 0x00,
        g: 0xD7,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "springgreen5",
        code: 41,
        r: 0x00,
        g: 0xD7,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "springgreen2",
        code: 42,
        r: 0x00,
        g: 0xD7,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "cyan3",
        code: 43,
        r: 0x00,
        g: 0xD7,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "darkturquoise",
        code: 44,
        r: 0x00,
        g: 0xD7,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "turquoise2",
        code: 45,
        r: 0x00,
        g: 0xD7,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "green1",
        code: 46,
        r: 0x00,
        g: 0xFF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "springgreen6",
        code: 47,
        r: 0x00,
        g: 0xFF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "springgreen1",
        code: 48,
        r: 0x00,
        g: 0xFF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "mediumspringgreen",
        code: 49,
        r: 0x00,
        g: 0xFF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "cyan2",
        code: 50,
        r: 0x00,
        g: 0xFF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "cyan1",
        code: 51,
        r: 0x00,
        g: 0xFF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "darkred1",
        code: 52,
        r: 0x5F,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "deeppink4",
        code: 53,
        r: 0x5F,
        g: 0x00,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "purple4",
        code: 54,
        r: 0x5F,
        g: 0x00,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "purple5",
        code: 55,
        r: 0x5F,
        g: 0x00,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "purple3",
        code: 56,
        r: 0x5F,
        g: 0x00,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "blueviolet",
        code: 57,
        r: 0x5F,
        g: 0x00,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "orange4",
        code: 58,
        r: 0x5F,
        g: 0x5F,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "grey37",
        code: 59,
        r: 0x5F,
        g: 0x5F,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "mediumpurple4",
        code: 60,
        r: 0x5F,
        g: 0x5F,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "slateblue3",
        code: 61,
        r: 0x5F,
        g: 0x5F,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "slateblue4",
        code: 62,
        r: 0x5F,
        g: 0x5F,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "royalblue1",
        code: 63,
        r: 0x5F,
        g: 0x5F,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "chartreuse4",
        code: 64,
        r: 0x5F,
        g: 0x87,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "darkseagreen4",
        code: 65,
        r: 0x5F,
        g: 0x87,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "paleturquoise4",
        code: 66,
        r: 0x5F,
        g: 0x87,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "steelblue",
        code: 67,
        r: 0x5F,
        g: 0x87,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "steelblue3",
        code: 68,
        r: 0x5F,
        g: 0x87,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "cornflowerblue",
        code: 69,
        r: 0x5F,
        g: 0x87,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "chartreuse3",
        code: 70,
        r: 0x5F,
        g: 0xAF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "darkseagreen5",
        code: 71,
        r: 0x5F,
        g: 0xAF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "cadetblue",
        code: 72,
        r: 0x5F,
        g: 0xAF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "cadetblue2",
        code: 73,
        r: 0x5F,
        g: 0xAF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "skyblue3",
        code: 74,
        r: 0x5F,
        g: 0xAF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "steelblue1",
        code: 75,
        r: 0x5F,
        g: 0xAF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "chartreuse5",
        code: 76,
        r: 0x5F,
        g: 0xD7,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "palegreen3",
        code: 77,
        r: 0x5F,
        g: 0xD7,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "seagreen3",
        code: 78,
        r: 0x5F,
        g: 0xD7,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "aquamarine3",
        code: 79,
        r: 0x5F,
        g: 0xD7,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "mediumturquoise",
        code: 80,
        r: 0x5F,
        g: 0xD7,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "steelblue2",
        code: 81,
        r: 0x5F,
        g: 0xD7,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "chartreuse2",
        code: 82,
        r: 0x5F,
        g: 0xFF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "seagreen2",
        code: 83,
        r: 0x5F,
        g: 0xFF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "seagreen1",
        code: 84,
        r: 0x5F,
        g: 0xFF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "seagreen4",
        code: 85,
        r: 0x5F,
        g: 0xFF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "aquamarine1",
        code: 86,
        r: 0x5F,
        g: 0xFF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "darkslategray2",
        code: 87,
        r: 0x5F,
        g: 0xFF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "darkred2",
        code: 88,
        r: 0x87,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "deeppink5",
        code: 89,
        r: 0x87,
        g: 0x00,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "darkmagenta",
        code: 90,
        r: 0x87,
        g: 0x00,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "darkmagenta2",
        code: 91,
        r: 0x87,
        g: 0x00,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "darkviolet1",
        code: 92,
        r: 0x87,
        g: 0x00,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "purple2",
        code: 93,
        r: 0x87,
        g: 0x00,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "orange5",
        code: 94,
        r: 0x87,
        g: 0x5F,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "lightpink4",
        code: 95,
        r: 0x87,
        g: 0x5F,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "plum4",
        code: 96,
        r: 0x87,
        g: 0x5F,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "mediumpurple3",
        code: 97,
        r: 0x87,
        g: 0x5F,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "mediumpurple5",
        code: 98,
        r: 0x87,
        g: 0x5F,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "slateblue1",
        code: 99,
        r: 0x87,
        g: 0x5F,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "yellow4",
        code: 100,
        r: 0x87,
        g: 0x87,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "wheat4",
        code: 101,
        r: 0x87,
        g: 0x87,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "grey53",
        code: 102,
        r: 0x87,
        g: 0x87,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "lightslategrey",
        code: 103,
        r: 0x87,
        g: 0x87,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "mediumpurple",
        code: 104,
        r: 0x87,
        g: 0x87,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "lightslateblue",
        code: 105,
        r: 0x87,
        g: 0x87,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "yellow5",
        code: 106,
        r: 0x87,
        g: 0xAF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "darkolivegreen3",
        code: 107,
        r: 0x87,
        g: 0xAF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "darkseagreen",
        code: 108,
        r: 0x87,
        g: 0xAF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "lightskyblue3",
        code: 109,
        r: 0x87,
        g: 0xAF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "lightskyblue4",
        code: 110,
        r: 0x87,
        g: 0xAF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "skyblue2",
        code: 111,
        r: 0x87,
        g: 0xAF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "chartreuse6",
        code: 112,
        r: 0x87,
        g: 0xD7,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "darkolivegreen4",
        code: 113,
        r: 0x87,
        g: 0xD7,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "palegreen4",
        code: 114,
        r: 0x87,
        g: 0xD7,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "darkseagreen3",
        code: 115,
        r: 0x87,
        g: 0xD7,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "darkslategray3",
        code: 116,
        r: 0x87,
        g: 0xD7,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "skyblue1",
        code: 117,
        r: 0x87,
        g: 0xD7,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "chartreuse1",
        code: 118,
        r: 0x87,
        g: 0xFF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "lightgreen1",
        code: 119,
        r: 0x87,
        g: 0xFF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "lightgreen2",
        code: 120,
        r: 0x87,
        g: 0xFF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "palegreen1",
        code: 121,
        r: 0x87,
        g: 0xFF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "aquamarine2",
        code: 122,
        r: 0x87,
        g: 0xFF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "darkslategray1",
        code: 123,
        r: 0x87,
        g: 0xFF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "red3",
        code: 124,
        r: 0xAF,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "deeppink6",
        code: 125,
        r: 0xAF,
        g: 0x00,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "mediumvioletred",
        code: 126,
        r: 0xAF,
        g: 0x00,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "magenta3",
        code: 127,
        r: 0xAF,
        g: 0x00,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "darkviolet2",
        code: 128,
        r: 0xAF,
        g: 0x00,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "purple6",
        code: 129,
        r: 0xAF,
        g: 0x00,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "darkorange3",
        code: 130,
        r: 0xAF,
        g: 0x5F,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "indianred1",
        code: 131,
        r: 0xAF,
        g: 0x5F,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "hotpink3",
        code: 132,
        r: 0xAF,
        g: 0x5F,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "mediumorchid3",
        code: 133,
        r: 0xAF,
        g: 0x5F,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "mediumorchid",
        code: 134,
        r: 0xAF,
        g: 0x5F,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "mediumpurple2",
        code: 135,
        r: 0xAF,
        g: 0x5F,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "darkgoldenrod",
        code: 136,
        r: 0xAF,
        g: 0x87,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "lightsalmon3",
        code: 137,
        r: 0xAF,
        g: 0x87,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "rosybrown",
        code: 138,
        r: 0xAF,
        g: 0x87,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "grey63",
        code: 139,
        r: 0xAF,
        g: 0x87,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "mediumpurple6",
        code: 140,
        r: 0xAF,
        g: 0x87,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "mediumpurple1",
        code: 141,
        r: 0xAF,
        g: 0x87,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "gold3",
        code: 142,
        r: 0xAF,
        g: 0xAF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "darkkhaki",
        code: 143,
        r: 0xAF,
        g: 0xAF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "navajowhite3",
        code: 144,
        r: 0xAF,
        g: 0xAF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "grey69",
        code: 145,
        r: 0xAF,
        g: 0xAF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "lightsteelblue3",
        code: 146,
        r: 0xAF,
        g: 0xAF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "lightsteelblue",
        code: 147,
        r: 0xAF,
        g: 0xAF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "yellow3",
        code: 148,
        r: 0xAF,
        g: 0xD7,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "darkolivegreen5",
        code: 149,
        r: 0xAF,
        g: 0xD7,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "darkseagreen6",
        code: 150,
        r: 0xAF,
        g: 0xD7,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "darkseagreen2",
        code: 151,
        r: 0xAF,
        g: 0xD7,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "lightcyan3",
        code: 152,
        r: 0xAF,
        g: 0xD7,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "lightskyblue1",
        code: 153,
        r: 0xAF,
        g: 0xD7,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "greenyellow",
        code: 154,
        r: 0xAF,
        g: 0xFF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "darkolivegreen2",
        code: 155,
        r: 0xAF,
        g: 0xFF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "palegreen2",
        code: 156,
        r: 0xAF,
        g: 0xFF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "darkseagreen7",
        code: 157,
        r: 0xAF,
        g: 0xFF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "darkseagreen1",
        code: 158,
        r: 0xAF,
        g: 0xFF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "paleturquoise1",
        code: 159,
        r: 0xAF,
        g: 0xFF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "red4",
        code: 160,
        r: 0xD7,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "deeppink3",
        code: 161,
        r: 0xD7,
        g: 0x00,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "deeppink7",
        code: 162,
        r: 0xD7,
        g: 0x00,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "magenta4",
        code: 163,
        r: 0xD7,
        g: 0x00,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "magenta5",
        code: 164,
        r: 0xD7,
        g: 0x00,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "magenta2",
        code: 165,
        r: 0xD7,
        g: 0x00,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "darkorange4",
        code: 166,
        r: 0xD7,
        g: 0x5F,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "indianred2",
        code: 167,
        r: 0xD7,
        g: 0x5F,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "hotpink4",
        code: 168,
        r: 0xD7,
        g: 0x5F,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "hotpink2",
        code: 169,
        r: 0xD7,
        g: 0x5F,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "orchid",
        code: 170,
        r: 0xD7,
        g: 0x5F,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "mediumorchid1",
        code: 171,
        r: 0xD7,
        g: 0x5F,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "orange3",
        code: 172,
        r: 0xD7,
        g: 0x87,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "lightsalmon4",
        code: 173,
        r: 0xD7,
        g: 0x87,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "lightpink3",
        code: 174,
        r: 0xD7,
        g: 0x87,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "pink3",
        code: 175,
        r: 0xD7,
        g: 0x87,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "plum3",
        code: 176,
        r: 0xD7,
        g: 0x87,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "violet",
        code: 177,
        r: 0xD7,
        g: 0x87,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "gold4",
        code: 178,
        r: 0xD7,
        g: 0xAF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "lightgoldenrod3",
        code: 179,
        r: 0xD7,
        g: 0xAF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "tan",
        code: 180,
        r: 0xD7,
        g: 0xAF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "mistyrose3",
        code: 181,
        r: 0xD7,
        g: 0xAF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "thistle3",
        code: 182,
        r: 0xD7,
        g: 0xAF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "plum2",
        code: 183,
        r: 0xD7,
        g: 0xAF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "yellow6",
        code: 184,
        r: 0xD7,
        g: 0xD7,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "khaki3",
        code: 185,
        r: 0xD7,
        g: 0xD7,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "lightgoldenrod2",
        code: 186,
        r: 0xD7,
        g: 0xD7,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "lightyellow3",
        code: 187,
        r: 0xD7,
        g: 0xD7,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "grey84",
        code: 188,
        r: 0xD7,
        g: 0xD7,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "lightsteelblue1",
        code: 189,
        r: 0xD7,
        g: 0xD7,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "yellow2",
        code: 190,
        r: 0xD7,
        g: 0xFF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "darkolivegreen1",
        code: 191,
        r: 0xD7,
        g: 0xFF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "darkolivegreen6",
        code: 192,
        r: 0xD7,
        g: 0xFF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "darkseagreen8",
        code: 193,
        r: 0xD7,
        g: 0xFF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "honeydew2",
        code: 194,
        r: 0xD7,
        g: 0xFF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "lightcyan1",
        code: 195,
        r: 0xD7,
        g: 0xFF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "red1",
        code: 196,
        r: 0xFF,
        g: 0x00,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "deeppink2",
        code: 197,
        r: 0xFF,
        g: 0x00,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "deeppink1",
        code: 198,
        r: 0xFF,
        g: 0x00,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "deeppink8",
        code: 199,
        r: 0xFF,
        g: 0x00,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "magenta6",
        code: 200,
        r: 0xFF,
        g: 0x00,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "magenta1",
        code: 201,
        r: 0xFF,
        g: 0x00,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "orangered1",
        code: 202,
        r: 0xFF,
        g: 0x5F,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "indianred3",
        code: 203,
        r: 0xFF,
        g: 0x5F,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "indianred4",
        code: 204,
        r: 0xFF,
        g: 0x5F,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "hotpink",
        code: 205,
        r: 0xFF,
        g: 0x5F,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "hotpink5",
        code: 206,
        r: 0xFF,
        g: 0x5F,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "mediumorchid2",
        code: 207,
        r: 0xFF,
        g: 0x5F,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "darkorange",
        code: 208,
        r: 0xFF,
        g: 0x87,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "salmon1",
        code: 209,
        r: 0xFF,
        g: 0x87,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "lightcoral",
        code: 210,
        r: 0xFF,
        g: 0x87,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "palevioletred1",
        code: 211,
        r: 0xFF,
        g: 0x87,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "orchid2",
        code: 212,
        r: 0xFF,
        g: 0x87,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "orchid1",
        code: 213,
        r: 0xFF,
        g: 0x87,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "orange1",
        code: 214,
        r: 0xFF,
        g: 0xAF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "sandybrown",
        code: 215,
        r: 0xFF,
        g: 0xAF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "lightsalmon1",
        code: 216,
        r: 0xFF,
        g: 0xAF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "lightpink1",
        code: 217,
        r: 0xFF,
        g: 0xAF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "pink1",
        code: 218,
        r: 0xFF,
        g: 0xAF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "plum1",
        code: 219,
        r: 0xFF,
        g: 0xAF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "gold1",
        code: 220,
        r: 0xFF,
        g: 0xD7,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "lightgoldenrod4",
        code: 221,
        r: 0xFF,
        g: 0xD7,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "lightgoldenrod5",
        code: 222,
        r: 0xFF,
        g: 0xD7,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "navajowhite1",
        code: 223,
        r: 0xFF,
        g: 0xD7,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "mistyrose1",
        code: 224,
        r: 0xFF,
        g: 0xD7,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "thistle1",
        code: 225,
        r: 0xFF,
        g: 0xD7,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "yellow1",
        code: 226,
        r: 0xFF,
        g: 0xFF,
        b: 0x00,
    },
    HighlightColorInfo {
        name: "lightgoldenrod1",
        code: 227,
        r: 0xFF,
        g: 0xFF,
        b: 0x5F,
    },
    HighlightColorInfo {
        name: "khaki1",
        code: 228,
        r: 0xFF,
        g: 0xFF,
        b: 0x87,
    },
    HighlightColorInfo {
        name: "wheat1",
        code: 229,
        r: 0xFF,
        g: 0xFF,
        b: 0xAF,
    },
    HighlightColorInfo {
        name: "cornsilk1",
        code: 230,
        r: 0xFF,
        g: 0xFF,
        b: 0xD7,
    },
    HighlightColorInfo {
        name: "grey100",
        code: 231,
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
    },
    HighlightColorInfo {
        name: "grey3",
        code: 232,
        r: 0x08,
        g: 0x08,
        b: 0x08,
    },
    HighlightColorInfo {
        name: "grey7",
        code: 233,
        r: 0x12,
        g: 0x12,
        b: 0x12,
    },
    HighlightColorInfo {
        name: "grey11",
        code: 234,
        r: 0x1C,
        g: 0x1C,
        b: 0x1C,
    },
    HighlightColorInfo {
        name: "grey15",
        code: 235,
        r: 0x26,
        g: 0x26,
        b: 0x26,
    },
    HighlightColorInfo {
        name: "grey19",
        code: 236,
        r: 0x30,
        g: 0x30,
        b: 0x30,
    },
    HighlightColorInfo {
        name: "grey23",
        code: 237,
        r: 0x3A,
        g: 0x3A,
        b: 0x3A,
    },
    HighlightColorInfo {
        name: "grey27",
        code: 238,
        r: 0x44,
        g: 0x44,
        b: 0x44,
    },
    HighlightColorInfo {
        name: "grey30",
        code: 239,
        r: 0x4E,
        g: 0x4E,
        b: 0x4E,
    },
    HighlightColorInfo {
        name: "grey35",
        code: 240,
        r: 0x58,
        g: 0x58,
        b: 0x58,
    },
    HighlightColorInfo {
        name: "grey39",
        code: 241,
        r: 0x62,
        g: 0x62,
        b: 0x62,
    },
    HighlightColorInfo {
        name: "grey42",
        code: 242,
        r: 0x6C,
        g: 0x6C,
        b: 0x6C,
    },
    HighlightColorInfo {
        name: "grey46",
        code: 243,
        r: 0x76,
        g: 0x76,
        b: 0x76,
    },
    HighlightColorInfo {
        name: "grey50",
        code: 244,
        r: 0x80,
        g: 0x80,
        b: 0x80,
    },
    HighlightColorInfo {
        name: "grey54",
        code: 245,
        r: 0x8A,
        g: 0x8A,
        b: 0x8A,
    },
    HighlightColorInfo {
        name: "grey58",
        code: 246,
        r: 0x94,
        g: 0x94,
        b: 0x94,
    },
    HighlightColorInfo {
        name: "grey62",
        code: 247,
        r: 0x9E,
        g: 0x9E,
        b: 0x9E,
    },
    HighlightColorInfo {
        name: "grey66",
        code: 248,
        r: 0xA8,
        g: 0xA8,
        b: 0xA8,
    },
    HighlightColorInfo {
        name: "grey70",
        code: 249,
        r: 0xB2,
        g: 0xB2,
        b: 0xB2,
    },
    HighlightColorInfo {
        name: "grey74",
        code: 250,
        r: 0xBC,
        g: 0xBC,
        b: 0xBC,
    },
    HighlightColorInfo {
        name: "grey78",
        code: 251,
        r: 0xC6,
        g: 0xC6,
        b: 0xC6,
    },
    HighlightColorInfo {
        name: "grey82",
        code: 252,
        r: 0xD0,
        g: 0xD0,
        b: 0xD0,
    },
    HighlightColorInfo {
        name: "grey85",
        code: 253,
        r: 0xDA,
        g: 0xDA,
        b: 0xDA,
    },
    HighlightColorInfo {
        name: "grey89",
        code: 254,
        r: 0xE4,
        g: 0xE4,
        b: 0xE4,
    },
    HighlightColorInfo {
        name: "grey93",
        code: 255,
        r: 0xEE,
        g: 0xEE,
        b: 0xEE,
    },
];
