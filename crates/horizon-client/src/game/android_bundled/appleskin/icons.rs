// AppleSkin mod — sprite grids and color palette.
// SPDX-License-Identifier: MIT; see LICENSE in this directory.

pub const SATURATION: [&[&str]; 4] = [
    &[
        ".........",
        ".........",
        ".........",
        ".........",
        "......a..",
        "......a..",
        ".....b.aa",
        "......b.a",
        "......bb.",
    ],
    &[
        "...c.....",
        "....d....",
        ".....a...",
        "......a..",
        "......a..",
        "......a..",
        "....bb.aa",
        "......b.a",
        "......bb.",
    ],
    &[
        "..cc.....",
        "....d....",
        ".....a...",
        "......a..",
        "......a..",
        "..b...a..",
        "...bbb.aa",
        "......b.a",
        "......bb.",
    ],
    &[
        "..cc.....",
        ".d..d....",
        "a....a...",
        "a.....a..",
        ".b....a..",
        "..b...a..",
        "...bbb.aa",
        "......b.a",
        "......bb.",
    ],
];

pub const HEART: &[&str] = &[
    "..ee.ee..",
    ".effeffe.",
    "efcggggfe",
    "eggggggge",
    "ehggggghe",
    ".ehggghe.",
    "..ehghe..",
    "...eie...",
    "....e....",
];

pub const FOOD: &[&str] = &[
    "..ee.....",
    ".efhe....",
    "efcfde...",
    "ehfdbde..",
    ".ejbbbe..",
    "..ejbbe..",
    "...eeecee",
    "......ece",
    "......ee.",
];

pub const PALETTE: [u32; 10] = [0xFFC2A100, 0xFF9F8609, 0xFFE9D262, 0xFFDDC241, 0xFF0A0A0A, 0xFFE01700, 0xFFFB1900, 0xFFAB1200, 0xFF9B1100, 0xFF282828];

pub fn color(ch: u8) -> u32 {
    PALETTE[usize::from(ch - b'a')]
}
