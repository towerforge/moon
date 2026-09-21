//! The Moon system's moon: the four-row header mark, derived from the
//! official 12 × 12 grid of `moon-mark.svg`. The half-block render is
//! generated from the grid, not written by hand.

/// The official grid, row 0 at the top. `#` = cell in `moon`. It is the mark
/// of the design system and what the header's moon is derived from.
pub const LOGO_GRID_FULL: [&str; 12] = [
    "...##.......",
    "..##........",
    ".###........",
    "####........",
    "####........",
    "####........",
    "#####.......",
    "######.....#",
    "############",
    ".##########.",
    "..########..",
    "...######...",
];

/// Left margin of the moon in the header, in columns: the same width as the
/// `❯ ` prefix of the messages, so that it lines up.
pub const LOGO_PAD: usize = 2;

/// The header's moon, 8 × 8 (four rows): a two-thirds reduction of the
/// official grid, adjusted by hand to keep its structure: horn at the top
/// right, thick body on the left, lower arc and the tip of the right horn one
/// row above the full row.
pub const LOGO_GRID: [&str; 8] = [
    "..#.....", ".##.....", "###.....", "###.....", "####...#", "########", ".######.", "..####..",
];

pub const LOGO_COLS: usize = 8;
pub const LOGO_ROWS: usize = 4;

/// Rows made of `▀ ▄ █`: each character is two pixels stacked vertically, and
/// since a cell is twice as tall as it is wide, each pixel comes out square.
fn half_blocks(grid: &[&str]) -> Vec<String> {
    grid.chunks(2)
        .map(|pair| {
            let top = pair[0].as_bytes();
            let bottom = pair[1].as_bytes();
            (0..top.len())
                .map(|c| match (top[c] == b'#', bottom[c] == b'#') {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                })
                .collect()
        })
        .collect()
}

/// The header's moon: four rows of eight characters.
pub fn logo_rows() -> Vec<String> {
    half_blocks(&LOGO_GRID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_checked_against_the_svg() {
        let celdas: usize = LOGO_GRID_FULL
            .iter()
            .map(|r| r.bytes().filter(|b| *b == b'#').count())
            .sum();
        assert_eq!(celdas, 67);
    }

    #[test]
    fn the_header_one_keeps_the_shape() {
        assert_eq!(logo_rows().len(), LOGO_ROWS);
        assert_eq!(
            logo_rows(),
            vec![" ▄█     ", "███     ", "████▄▄▄█", " ▀████▀ "]
        );
        // full row, and the tip of the right horn right above it
        assert_eq!(LOGO_GRID[5], "########");
        assert!(LOGO_GRID[4].ends_with('#'));
        // the moon opens toward the top right: the first row starts to the right of the edge
        assert!(LOGO_GRID[0].starts_with(".."));
    }
}
