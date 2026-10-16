//! Évaluation statique : distance de connexion pondérée par la gravité.

use crate::game::{COL0, COL8, N, Position, ROW0, ROW8, largest_zone, spread};
use crate::pieces::{SHAPE_SIZES, SHAPES};

#[derive(Debug, Clone, Copy)]
pub struct Weights {
    /// Coût d'une case vide posée directement sur la surface.
    pub empty: i32,
    /// Surcoût par case vide qu'il faudrait combler dessous.
    pub gap: i32,
    /// Poids d'un point d'écart de distance.
    pub distance: i32,
    /// Poids de l'axe secondaire, en seizièmes.
    pub second_axis: i32,
    /// Poids d'une case d'écart de plus grande zone, en fin de partie.
    pub zone: i32,
    pub tempo: i32,
}

pub const DEFAULT_WEIGHTS: Weights =
    Weights { empty: 4, gap: 3, distance: 40, second_axis: 4, zone: 30, tempo: 60 };

pub const UNREACHABLE: i32 = 400;

pub fn remaining_cells(pos: &Position, p: usize) -> i32 {
    (0..SHAPES).map(|s| pos.inv[p][s] as i32 * SHAPE_SIZES[s] as i32).sum()
}

const COL_FIRST: u128 = COL0;
const COL_LAST: u128 = COL8;
const ROW_FIRST: u128 = ROW0;
const ROW_LAST: u128 = ROW8;

/// Plus court chemin d'un bord à l'opposé pour `p`, sur les deux axes.
/// Résultat : (gauche-droite, haut-bas), en unités de coût.
pub fn distances(pos: &Position, p: usize, w: &Weights) -> (i32, i32) {
    let own = pos.bb[p];
    let blocked = pos.bb[1 - p];
    // classes[g] : cases vides avec `g` cases vides dessous.
    let mut classes = [0u128; N];
    for x in 0..N {
        let surface = 8 - pos.heights[x] as i32;
        for y in 0..=surface {
            classes[(surface - y) as usize] |= 1u128 << (y as usize * N + x);
        }
    }
    let costs: [i32; N] = std::array::from_fn(|g| w.empty + w.gap * g as i32);
    let lr = shortest(own, blocked, &classes, &costs, COL_FIRST, COL_LAST);
    let tb = shortest(own, blocked, &classes, &costs, ROW_FIRST, ROW_LAST);
    (lr, tb)
}

const BUCKETS: usize = UNREACHABLE as usize + 64;

fn shortest(own: u128, blocked: u128, classes: &[u128; N], costs: &[i32; N], start: u128, goal: u128) -> i32 {
    let mut buckets = [0u128; BUCKETS];
    buckets[0] = start & own;
    for g in 0..N {
        let c = costs[g] as usize;
        if c < BUCKETS {
            buckets[c] |= start & classes[g];
        }
    }
    let mut settled = blocked;
    for d in 0..UNREACHABLE as usize {
        let mut cur = buckets[d] & !settled;
        if cur == 0 {
            continue;
        }
        loop {
            let next = cur | (spread(cur) & own & !settled);
            if next == cur {
                break;
            }
            cur = next;
        }
        if cur & goal != 0 {
            return d as i32;
        }
        settled |= cur;
        let frontier = spread(cur) & !settled;
        for g in 0..N {
            let reach = frontier & classes[g];
            if reach != 0 {
                let nd = d + costs[g] as usize;
                if nd < BUCKETS {
                    buckets[nd] |= reach;
                }
            }
        }
    }
    UNREACHABLE
}

fn connection_score(pos: &Position, p: usize, w: &Weights) -> i32 {
    let (lr, tb) = distances(pos, p, w);
    let (best, other) = if lr <= tb { (lr, tb) } else { (tb, lr) };
    let mut d = best * 16 + other * w.second_axis;
    if best >= UNREACHABLE || best / w.empty.max(1) > remaining_cells(pos, p) {
        d = UNREACHABLE * 20;
    }
    d
}

/// Score pour le joueur au trait.
pub fn evaluate(pos: &Position, w: &Weights) -> i32 {
    let p = pos.to_move;
    let q = 1 - p;
    let dist = (connection_score(pos, q, w) - connection_score(pos, p, w)) * w.distance / 16;
    let filled = pos.occupied().count_ones() as i32;
    let zones = largest_zone(pos.bb[p]) as i32 - largest_zone(pos.bb[q]) as i32;
    dist + zones * w.zone * filled / 81 + w.tempo
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::BLUE;
    use crate::notation::parse_record;

    #[test]
    fn empty_board_distances_are_symmetric() {
        let pos = Position::new(BLUE);
        let (lr, tb) = distances(&pos, BLUE, &DEFAULT_WEIGHTS);
        assert_eq!(lr, 9 * DEFAULT_WEIGHTS.empty);
        assert!(tb > lr);
    }

    #[test]
    fn own_floor_row_shortens_distance() {
        let pos = parse_record("3I1 11 3I4").unwrap();
        let (lr, _) = distances(&pos, BLUE, &DEFAULT_WEIGHTS);
        assert_eq!(lr, 3 * DEFAULT_WEIGHTS.empty);
    }
}
