//! The color-only ANSI subset consumed by the Notcurses backend.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalColors {
    pub(crate) fg: (u8, u8, u8),
    pub(crate) bg: (u8, u8, u8),
}

impl Default for TerminalColors {
    fn default() -> Self {
        Self {
            fg: (208, 208, 208),
            bg: (16, 16, 24),
        }
    }
}

impl TerminalColors {
    /// Apply complete SGR parameters, without the CSI prefix or final `m`.
    /// Malformed color groups leave the entire sequence unapplied so their
    /// components cannot accidentally become resets or other SGR commands.
    pub(crate) fn apply_sgr(self, parameters: &str) -> Self {
        let mut next = self;
        let mut parameters = parameters.split(';');
        while let Some(parameter) = parameters.next() {
            let code = if parameter.is_empty() {
                0
            } else if let Some(code) = decimal(parameter) {
                code
            } else {
                return self;
            };
            match code {
                0 => next = Self::default(),
                39 => next.fg = Self::default().fg,
                49 => next.bg = Self::default().bg,
                38 | 48 => {
                    let color = match parameters.next() {
                        Some("2") => {
                            let rgb = (
                                parameters.next().and_then(decimal),
                                parameters.next().and_then(decimal),
                                parameters.next().and_then(decimal),
                            );
                            match rgb {
                                (Some(r), Some(g), Some(b)) => (r, g, b),
                                _ => return self,
                            }
                        }
                        Some("5") => match parameters.next().and_then(decimal) {
                            Some(index) => ansi256_to_rgb(index),
                            None => return self,
                        },
                        _ => return self,
                    };
                    if code == 38 {
                        next.fg = color;
                    } else {
                        next.bg = color;
                    }
                }
                _ => {} // Non-color styles are deliberately ignored.
            }
        }
        next
    }
}

fn decimal(parameter: &str) -> Option<u8> {
    if parameter.is_empty() || !parameter.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    parameter.parse().ok()
}

/// Consume an escape after its initial ESC byte. Only a complete CSI ... m
/// returns SGR parameters; other controls are skipped without becoming glyphs.
/// Slices always end on ASCII boundaries, including when input contains Unicode.
pub(crate) fn consume_escape(input: &str) -> (&str, Option<&str>) {
    let Some(first) = input.as_bytes().first() else {
        return (input, None);
    };
    match first {
        b'[' => {
            for (index, byte) in input.bytes().enumerate().skip(1) {
                if byte == 0x1b {
                    // An interrupted sequence must not swallow the next escape.
                    return (&input[index..], None);
                }
                if (0x40..=0x7e).contains(&byte) {
                    let parameters = (byte == b'm').then_some(&input[1..index]);
                    return (&input[index + 1..], parameters);
                }
            }
        }
        b']' | b'P' | b'X' | b'^' | b'_' => {
            // OSC terminates with BEL or ST; other control strings use ST.
            for (index, byte) in input.bytes().enumerate().skip(1) {
                if (*first == b']' && byte == 0x07)
                    || (byte == b'\\' && input.as_bytes()[index - 1] == 0x1b)
                {
                    return (&input[index + 1..], None);
                }
            }
        }
        _ => {
            // Other ESC sequences have zero or more intermediate bytes and
            // one final byte (e.g. ESC ( B, ESC 7, ESC 8).
            for (index, byte) in input.bytes().enumerate() {
                if byte == 0x1b {
                    return (&input[index..], None);
                }
                if (0x30..=0x7e).contains(&byte) {
                    return (&input[index + 1..], None);
                }
            }
        }
    }
    ("", None) // Truncated controls have no visible text or color effect.
}

fn ansi256_to_rgb(code: u8) -> (u8, u8, u8) {
    match code {
        0 => (0, 0, 0),
        1 => (128, 0, 0),
        2 => (0, 128, 0),
        3 => (128, 128, 0),
        4 => (0, 0, 128),
        5 => (128, 0, 128),
        6 => (0, 128, 128),
        7 => (192, 192, 192),
        8 => (128, 128, 128),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let n = code - 16;
            let r = (n / 36) % 6;
            let g = (n / 6) % 6;
            let b = n % 6;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (step(r), step(g), step(b))
        }
        232..=255 => {
            let gray = 8 + (code - 232) * 10;
            (gray, gray, gray)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_truecolor_foreground_and_background() {
        let (text, parameters) = consume_escape("[38;2;255;128;0;48;2;12;34;56m▀");
        let colors = TerminalColors::default().apply_sgr(parameters.unwrap());
        assert_eq!(text, "▀");
        assert_eq!(colors.fg, (255, 128, 0));
        assert_eq!(colors.bg, (12, 34, 56));
        assert_eq!(
            TerminalColors::default()
                .apply_sgr("38;2;255;128;0")
                .apply_sgr("48;2;12;34;56"),
            colors
        );
    }

    #[test]
    fn reset_restores_both_channels_and_obeys_parameter_order() {
        let colors = TerminalColors::default().apply_sgr("38;2;1;2;3;48;2;4;5;6");
        assert_eq!(colors.apply_sgr("0"), TerminalColors::default());
        assert_eq!(colors.apply_sgr(""), TerminalColors::default());
        assert_eq!(
            colors.apply_sgr("0;48;2;7;8;9"),
            TerminalColors {
                bg: (7, 8, 9),
                ..TerminalColors::default()
            }
        );
        assert_eq!(colors.apply_sgr("38;2;1;2;3;0"), TerminalColors::default());
    }

    #[test]
    fn selective_resets_preserve_the_other_channel() {
        let colors = TerminalColors::default().apply_sgr("38;2;1;2;3;48;2;4;5;6");
        assert_eq!(
            colors.apply_sgr("39"),
            TerminalColors {
                fg: TerminalColors::default().fg,
                ..colors
            }
        );
        assert_eq!(
            colors.apply_sgr("49"),
            TerminalColors {
                bg: TerminalColors::default().bg,
                ..colors
            }
        );
        assert_eq!(colors.apply_sgr("39;49"), TerminalColors::default());
    }

    #[test]
    fn indexed_colors_support_both_channels_and_keep_the_existing_palette() {
        let colors = TerminalColors::default().apply_sgr("38;5;196;48;5;232");
        assert_eq!(colors.fg, (255, 0, 0));
        assert_eq!(colors.bg, (8, 8, 8));
        for (index, rgb) in [
            (0, (0, 0, 0)),
            (7, (192, 192, 192)),
            (15, (255, 255, 255)),
            (16, (0, 0, 0)),
            (59, (95, 95, 95)),
            (231, (255, 255, 255)),
            (255, (238, 238, 238)),
        ] {
            assert_eq!(ansi256_to_rgb(index), rgb);
        }
        assert_eq!(colors.apply_sgr("38;5;21").bg, colors.bg);
    }

    #[test]
    fn malformed_sgr_is_ignored_without_partial_changes() {
        let colors = TerminalColors::default().apply_sgr("38;2;1;2;3;48;2;4;5;6");
        for parameters in [
            "38",
            "48;2",
            "38;2;1;2",
            "38;2;1;;3",
            "48;5",
            "38;5;256",
            "48;2;0;300;0",
            "38;2;-1;0;0",
            "38;2;+1;0;0",
            "38;3;0",
            "38;2;999999999999999999999;0;0",
            "38;2;é;0;0",
            "0;38;5",
            "38;2;1;2;3;48;2;4;5",
            "?25",
            "38:2:1:2:3",
        ] {
            assert_eq!(colors.apply_sgr(parameters), colors, "{parameters:?}");
        }
        assert_eq!(colors.apply_sgr("1;4;22;24"), colors);
    }

    #[test]
    fn non_color_and_truncated_escapes_are_safely_skipped() {
        for escape in [
            "[K",
            "[2J",
            "[?25l",
            "[1;2H",
            "7",
            "(B",
            "]0;title\x07",
            "]0;title\x1b\\",
            "Pignored\x1b\\",
        ] {
            let input = format!("{escape}▀");
            assert_eq!(consume_escape(&input), ("▀", None), "{escape:?}");
        }
        for truncated in ["", "[", "[38;2;1;2", "[48;5;", "[38;2;é", "]title", "("] {
            assert_eq!(consume_escape(truncated), ("", None), "{truncated:?}");
        }
        assert_eq!(consume_escape("[38;2;1;\x1b[0m"), ("\x1b[0m", None));
    }
}
