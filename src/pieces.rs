//! Formes, orientations distinctes et leur écriture canonique.

use std::sync::OnceLock;

pub const SHAPES: usize = 7;
pub const SHAPE_NAMES: [&str; SHAPES] = ["1", "2", "3I", "3L", "4S", "4T", "4L"];
pub const SHAPE_SIZES: [u32; SHAPES] = [1, 2, 3, 3, 4, 4, 4];

const BASE: [&[&str]; SHAPES] = [
    &["X"],
    &["XX"],
    &["XXX"],
    &["XX", "X."],
    &[".X", "XX", "X."],
    &["XXX", ".X."],
    &["XXX", "X.."],
];

pub type Cells = Vec<(i32, i32)>;

#[derive(Debug, Clone)]
pub struct Orientation {
    /// Cases normalisées (x vers la droite, y vers le bas), triées.
    pub cells: Cells,
    pub width: usize,
    pub height: usize,
    /// Rotation et miroir de l'écriture canonique.
    pub rotation: u8,
    pub flipped: bool,
    /// Pour chaque colonne de la pièce : plus haute et plus basse case.
    pub col_top: [u8; 4],
    pub col_bottom: [u8; 4],
    /// Dénivelé que la pose exige entre la colonne d'ancrage et les deux
    /// suivantes (`col_bottom[0] - col_bottom[c]`), et masque des colonnes
    /// que la pièce occupe réellement (`-1`) ou non (`0`).
    pub dd: [i8; 2],
    pub dm: [i8; 2],
    /// Masque des cases, ancre en (0, 0), index `y * 9 + x`.
    pub mask: u128,
}

pub fn normalize(cells: &[(i32, i32)]) -> Cells {
    let min_x = cells.iter().map(|c| c.0).min().unwrap();
    let min_y = cells.iter().map(|c| c.1).min().unwrap();
    let mut out: Cells = cells.iter().map(|&(x, y)| (x - min_x, y - min_y)).collect();
    out.sort_by_key(|&(x, y)| (y, x));
    out
}

pub fn base_cells(shape: usize) -> Cells {
    let mut cells = Vec::new();
    for (y, row) in BASE[shape].iter().enumerate() {
        for (x, ch) in row.chars().enumerate() {
            if ch == 'X' {
                cells.push((x as i32, y as i32));
            }
        }
    }
    normalize(&cells)
}

/// Miroir d'abord, puis `rotation` quarts de tour horaires.
pub fn transform(shape: usize, rotation: u8, flipped: bool) -> Cells {
    let mut cells = base_cells(shape);
    if flipped {
        cells = normalize(&cells.iter().map(|&(x, y)| (-x, y)).collect::<Vec<_>>());
    }
    for _ in 0..rotation {
        cells = normalize(&cells.iter().map(|&(x, y)| (-y, x)).collect::<Vec<_>>());
    }
    cells
}

fn build(cells: Cells, rotation: u8, flipped: bool) -> Orientation {
    let width = cells.iter().map(|c| c.0).max().unwrap() as usize + 1;
    let height = cells.iter().map(|c| c.1).max().unwrap() as usize + 1;
    let mut col_top = [u8::MAX; 4];
    let mut col_bottom = [0u8; 4];
    let mut mask = 0u128;
    for &(x, y) in &cells {
        let (x, y) = (x as usize, y as usize);
        col_top[x] = col_top[x].min(y as u8);
        col_bottom[x] = col_bottom[x].max(y as u8);
        mask |= 1u128 << (y * 9 + x);
    }
    let mut dd = [0i8; 2];
    let mut dm = [0i8; 2];
    for c in 1..width.min(3) {
        dd[c - 1] = col_bottom[0] as i8 - col_bottom[c] as i8;
        dm[c - 1] = -1;
    }
    Orientation {
        cells,
        width,
        height,
        rotation,
        flipped,
        col_top,
        col_bottom,
        dd,
        dm,
        mask,
    }
}

fn compute() -> Vec<Vec<Orientation>> {
    (0..SHAPES)
        .map(|shape| {
            let mut list: Vec<Orientation> = Vec::new();
            for flipped in [false, true] {
                for rotation in 0..4u8 {
                    let cells = transform(shape, rotation, flipped);
                    if !list.iter().any(|o| o.cells == cells) {
                        list.push(build(cells, rotation, flipped));
                    }
                }
            }
            list
        })
        .collect()
}

pub fn orientations(shape: usize) -> &'static [Orientation] {
    static CACHE: OnceLock<Vec<Vec<Orientation>>> = OnceLock::new();
    &CACHE.get_or_init(compute)[shape]
}

/// Indice de l'orientation distincte qui a cette géométrie.
pub fn orientation_index(shape: usize, rotation: u8, flipped: bool) -> usize {
    let cells = transform(shape, rotation % 4, flipped);
    orientations(shape)
        .iter()
        .position(|o| o.cells == cells)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_orientation_counts() {
        let counts: Vec<usize> = (0..SHAPES).map(|s| orientations(s).len()).collect();
        assert_eq!(counts, vec![1, 2, 2, 4, 4, 4, 8]);
    }

    #[test]
    fn sizes_and_totals() {
        for (s, &size) in SHAPE_SIZES.iter().enumerate() {
            assert_eq!(base_cells(s).len() as u32, size);
        }
        assert_eq!(SHAPE_SIZES.iter().sum::<u32>() * 2, 42);
    }

    #[test]
    fn clockwise_rotation_of_t_points_stem_left() {
        // T : barre en haut, tige en bas. Un quart horaire met la barre à droite.
        let t1 = transform(5, 1, false);
        assert_eq!(t1, normalize(&[(1, 0), (0, 1), (1, 1), (1, 2)]));
    }
}
