//! Compact ASCII atlas for Veyrath — The Dying Crown.
//!
//! The source checkpoint stays editable in Azgaar. This derived atlas is the
//! terminal-native view used by the in-game World Map modal; it deliberately
//! uses only printable ASCII so it remains legible in plain terminals.

pub const ASCII_ATLAS: &str = include_str!("../../assets/maps/veyrath/veyrath-04-ascii.txt");

pub const REGION_LEGEND: &[(&str, char, (u8, u8, u8))] = &[
    ("Ashen Synod", 'a', (190, 195, 205)),
    ("House Gallowmere", 'g', (110, 165, 135)),
    ("Crown of Veyrath", 'c', (205, 190, 155)),
    ("Greyfall Compact", 'v', (180, 165, 130)),
    ("Mire Courts", 'm', (125, 170, 180)),
    ("Blackwake League", 'b', (120, 155, 175)),
];

pub const ATLAS_NOTE: &str = "Millhaven (*): starter settlement in Greyfall Vale";

pub fn atlas_lines(max_width: usize, max_height: usize) -> Vec<String> {
    ASCII_ATLAS
        .lines()
        .take(max_height)
        .map(|line| line.chars().take(max_width).collect())
        .collect()
}

pub fn glyph_color(glyph: char) -> (u8, u8, u8) {
    match glyph {
        'a' => (190, 195, 205),
        'g' => (110, 165, 135),
        'c' => (205, 190, 155),
        'v' => (180, 165, 130),
        'm' => (125, 170, 180),
        'b' => (120, 155, 175),
        'A' | 'D' | 'G' | 'M' | 'B' => (255, 215, 120),
        '@' => (255, 255, 255),
        '*' => (120, 255, 160),
        '^' => (175, 180, 190),
        '~' => (70, 135, 195),
        ':' => (85, 180, 215),
        '=' => (205, 155, 90),
        '.' => (145, 135, 125),
        '+' | '-' | '|' => (90, 120, 100),
        _ => (150, 155, 165),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_is_printable_and_contains_the_checkpoint_shape() {
        assert!(ASCII_ATLAS.lines().count() >= 16);
        assert!(ASCII_ATLAS.lines().all(|line| line.is_ascii()));
        assert!(ASCII_ATLAS.contains('@'));
        assert!(ASCII_ATLAS.contains('*'));
        assert!(ASCII_ATLAS.contains('~'));
        assert!(ASCII_ATLAS.contains('^'));
    }

    #[test]
    fn atlas_lines_fit_the_terminal_map_partition() {
        let lines = atlas_lines(38, 16);
        assert_eq!(lines.len(), 16);
        assert!(lines.iter().all(|line| line.chars().count() <= 38));
    }
}
