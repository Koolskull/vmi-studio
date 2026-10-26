//! The six studio themes. Paper is Studio with every channel inverted.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub field: [u8; 3],
    pub ink: [u8; 3],
    pub quiet: [u8; 3],
    pub rule: [u8; 3],
    pub row: [u8; 3],
    pub gutter: [u8; 3],
    pub grid: [u8; 3],
    pub frame: [u8; 3],
    pub soft: [u8; 3],
    pub title: [u8; 3],
    pub number: [u8; 3],
}

const fn invert(color: [u8; 3]) -> [u8; 3] {
    [255 - color[0], 255 - color[1], 255 - color[2]]
}

const STUDIO: Theme = Theme {
    id: "studio",
    name: "Studio",
    blurb: "Black field. White on the current row.",
    field: [0, 0, 0],
    ink: [255, 255, 255],
    quiet: [0x3a, 0x3a, 0x3a],
    rule: [0x2a, 0x2a, 0x2a],
    row: [0x11, 0x11, 0x11],
    gutter: [0x14, 0x14, 0x14],
    grid: [0x04, 0x04, 0x04],
    frame: [0x33, 0x33, 0x33],
    soft: [0xa0, 0xa0, 0xa0],
    title: [0x2a, 0x2a, 0x2a],
    number: [0xa0, 0xa0, 0xa0],
};

const PAPER: Theme = Theme {
    id: "paper",
    name: "Paper",
    blurb: "Studio, inverted, for a light field.",
    field: invert(STUDIO.field),
    ink: invert(STUDIO.ink),
    quiet: invert(STUDIO.quiet),
    rule: invert(STUDIO.rule),
    row: invert(STUDIO.row),
    gutter: invert(STUDIO.gutter),
    grid: invert(STUDIO.grid),
    frame: invert(STUDIO.frame),
    soft: invert(STUDIO.soft),
    title: invert(STUDIO.title),
    number: invert(STUDIO.number),
};

const CHARCOAL: Theme = Theme {
    id: "charcoal",
    name: "Charcoal",
    blurb: "High contrast charcoal with a blue frame.",
    field: [0, 0, 0],
    ink: [255, 255, 255],
    quiet: [0x43, 0xb9, 0xd8],
    rule: [0x43, 0xb9, 0xd8],
    row: [0x16, 0x38, 0x44],
    gutter: [0x0c, 0x24, 0x30],
    grid: [0x07, 0x18, 0x20],
    frame: [0x1d, 0x4e, 0x62],
    soft: [0x7e, 0xc8, 0xe0],
    title: [0x43, 0xb9, 0xd8],
    number: [0x7e, 0xc8, 0xe0],
};

const PEACH: Theme = Theme {
    id: "peach",
    name: "Peach",
    blurb: "Warm light pastel.",
    field: [0xff, 0xf1, 0xe8],
    ink: [0x7a, 0x4a, 0x3a],
    quiet: [0xc4, 0xa0, 0x90],
    rule: [0xe7, 0xcf, 0xc2],
    row: [0xff, 0xe4, 0xd6],
    gutter: [0xf3, 0xdd, 0xd0],
    grid: [0xf8, 0xe6, 0xda],
    frame: [0xed, 0xd5, 0xc6],
    soft: [0xa8, 0x84, 0x74],
    title: [0xd7, 0xb5, 0xa4],
    number: [0xa8, 0x84, 0x74],
};

const MINT: Theme = Theme {
    id: "mint",
    name: "Mint",
    blurb: "Cool light pastel.",
    field: [0xee, 0xf8, 0xf2],
    ink: [0x3d, 0x6b, 0x58],
    quiet: [0x9b, 0xb8, 0xaa],
    rule: [0xcf, 0xe3, 0xd8],
    row: [0xdf, 0xf3, 0xe8],
    gutter: [0xd5, 0xeb, 0xe0],
    grid: [0xe4, 0xf3, 0xeb],
    frame: [0xc5, 0xdd, 0xd0],
    soft: [0x6f, 0x9a, 0x86],
    title: [0xb7, 0xd4, 0xc6],
    number: [0x6f, 0x9a, 0x86],
};

const LILAC: Theme = Theme {
    id: "lilac",
    name: "Lilac",
    blurb: "Soft light pastel.",
    field: [0xf6, 0xf0, 0xfa],
    ink: [0x6a, 0x50, 0x80],
    quiet: [0xb4, 0xa3, 0xc4],
    rule: [0xe2, 0xd4, 0xee],
    row: [0xef, 0xe4, 0xf7],
    gutter: [0xe6, 0xd8, 0xf0],
    grid: [0xf0, 0xe6, 0xf6],
    frame: [0xdd, 0xd0, 0xea],
    soft: [0x91, 0x78, 0xa8],
    title: [0xcb, 0xb8, 0xdc],
    number: [0x91, 0x78, 0xa8],
};

pub const THEMES: &[Theme] = &[STUDIO, PAPER, CHARCOAL, PEACH, MINT, LILAC];

pub fn theme(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|item| item.id == id)
}

pub fn theme_or_studio(id: &str) -> &'static Theme {
    theme(id).unwrap_or(&STUDIO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_is_the_studio_inverted() {
        let paper = theme("paper").unwrap();
        assert_eq!(paper.field, invert(STUDIO.field));
        assert_eq!(paper.ink, invert(STUDIO.ink));
        assert_eq!(paper.quiet, invert(STUDIO.quiet));
        assert_eq!(THEMES.len(), 6);
        assert_eq!(theme_or_studio("missing").id, "studio");
    }
}
