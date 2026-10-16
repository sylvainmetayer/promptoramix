//! Évaluation statique : distance de connexion pondérée par la gravité.

use crate::game::{COL0, COL8, FULL, N, Position, ROW0, ROW8, largest_zone, spread};
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
    /// Surcoût d'une case étroite, selon qu'il reste ou non une pièce étroite.
    pub narrow_ok: i32,
    pub narrow_none: i32,
    /// Puits profonds infranchissables sans pièce étroite.
    pub deep_wells: bool,
    /// Le grand L comble un puits de deux cases.
    pub wells3: bool,
}

pub const DEFAULT_WEIGHTS: Weights = Weights {
    empty: 4,
    gap: 3,
    distance: 40,
    second_axis: 12,
    zone: 30,
    tempo: 60,
    narrow_ok: 0,
    narrow_none: 0,
    deep_wells: true,
    wells3: false,
};

pub const UNREACHABLE: i32 = 400;

pub fn remaining_cells(pos: &Position, p: usize) -> i32 {
    (0..SHAPES)
        .map(|s| pos.inv[p][s] as i32 * SHAPE_SIZES[s] as i32)
        .sum()
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
    let occ = pos.occupied();
    // Case vide coincée entre deux cases pleines ou un bord : seule une pièce
    // étroite (mono, domino, barre) peut encore s'y loger sans appui latéral.
    let narrow = !occ & (COL0 | ((occ & !COL8) << 1)) & (COL8 | ((occ & !COL0) >> 1)) & FULL;
    let has_narrow = pos.inv[p][0] + pos.inv[p][1] + pos.inv[p][2] > 0;
    let narrow_cost = if has_narrow {
        w.narrow_ok
    } else {
        w.narrow_none
    };
    // Puits d'au moins deux cases de profondeur : sans pièce étroite, rien ne
    // peut plus y descendre, la case est perdue pour ce joueur. Sauf le grand
    // L, dont la branche de trois descend dans un puits de deux, le bras posé
    // sur le voisin : avec un grand L en réserve, seuls les puits de trois
    // cases et plus sont perdus.
    let mut deep = narrow & (narrow << N);
    if w.wells3 && pos.inv[p][6] > 0 {
        deep &= narrow << (2 * N);
    }
    for _ in 0..N {
        deep |= (deep >> N) & !occ;
    }
    let blocked = if has_narrow || !w.deep_wells {
        blocked
    } else {
        blocked | deep
    };
    // classes[g] : cases vides avec `g` cases vides dessous ; +N si étroites.
    let mut classes = [0u128; K];
    for x in 0..N {
        let surface = 8 - pos.heights[x] as i32;
        for y in 0..=surface {
            classes[(surface - y) as usize] |= 1u128 << (y as usize * N + x);
        }
    }
    for g in 0..N {
        classes[g + N] = classes[g] & narrow;
        classes[g] &= !narrow;
    }
    let costs: [i32; K] = std::array::from_fn(|k| {
        w.empty + w.gap * (k % N) as i32 + if k >= N { narrow_cost } else { 0 }
    });
    // Seules les classes non vides et non bloquées comptent dans la boucle.
    let mut active = [(0u128, 0usize); K];
    let mut n = 0;
    for k in 0..K {
        let cells = classes[k] & !blocked;
        if cells != 0 {
            active[n] = (cells, (costs[k].max(0) as usize).min(RING - 1));
            n += 1;
        }
    }
    let active = &active[..n];
    let lr = shortest(own, blocked, active, COL_FIRST, COL_LAST);
    let tb = shortest(own, blocked, active, ROW_FIRST, ROW_LAST);
    (lr, tb)
}

/// File de Dial circulaire : tout coût d'arête est inférieur à `RING`, donc
/// une case n'écrase jamais un seau pas encore traité.
const RING: usize = 64;

const K: usize = 2 * N;

fn shortest(own: u128, blocked: u128, active: &[(u128, usize)], start: u128, goal: u128) -> i32 {
    let mut ring = [0u128; RING];
    let mut pending = 0u64;
    let push = |ring: &mut [u128; RING], pending: &mut u64, at: usize, cells: u128| {
        if cells != 0 {
            ring[at % RING] |= cells;
            *pending |= 1 << (at % RING);
        }
    };
    push(&mut ring, &mut pending, 0, start & own);
    for &(cells, step) in active {
        push(&mut ring, &mut pending, step, start & cells);
    }
    let mut settled = blocked;
    let mut d = 0usize;
    while pending != 0 && d < UNREACHABLE as usize {
        d += pending.rotate_right((d % RING) as u32).trailing_zeros() as usize;
        let slot = d % RING;
        pending &= !(1 << slot);
        let mut cur = std::mem::take(&mut ring[slot]) & !settled;
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
            return (d as i32).min(UNREACHABLE);
        }
        settled |= cur;
        let frontier = spread(cur) & !settled;
        for &(cells, step) in active {
            push(&mut ring, &mut pending, d + step, frontier & cells);
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
    fn deep_well_blocks_player_without_narrow_pieces() {
        // Fin d'une partie perdue au départage : bleu n'a plus de pièce étroite
        // et ses chemins ne passent plus que par les puits des colonnes 1, 3 et 9.
        let pos = parse_record(
            "4Tr21 4Tr25 3Ir12 2r16 3Ir12 12 2r16 4Sr17 4Lr37 4Lr17 3Lr36 3Lr16 \
             4Tr34 3Ir18 2r19 4Ss4 4Ss4 2r14 15 13 15",
        )
        .unwrap();
        assert_eq!(pos.inv[BLUE][..3], [0, 0, 0]);
        let naive = distances(
            &pos,
            BLUE,
            &Weights {
                deep_wells: false,
                ..DEFAULT_WEIGHTS
            },
        );
        let aware = distances(
            &pos,
            BLUE,
            &Weights {
                deep_wells: true,
                ..DEFAULT_WEIGHTS
            },
        );
        assert!(naive.0 < UNREACHABLE && naive.1 < UNREACHABLE);
        assert_eq!(aware, (UNREACHABLE, UNREACHABLE));
    }

    #[test]
    fn own_floor_row_shortens_distance() {
        let pos = parse_record("3I1 11 3I4").unwrap();
        let (lr, _) = distances(&pos, BLUE, &DEFAULT_WEIGHTS);
        assert_eq!(lr, 3 * DEFAULT_WEIGHTS.empty);
    }
}
