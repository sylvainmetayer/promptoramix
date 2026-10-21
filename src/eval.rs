//! Évaluation statique : distance de connexion pondérée par la gravité.

use crate::game::{COL0, COL8, FULL, N, Position, ROW0, ROW8, flood, spread};
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
    /// Pas de saut de score quand un axe devient inatteignable.
    pub cliff: bool,
    /// Poids des zones renforcé en phase de blocage.
    pub zone_phase: bool,
    /// Une case au-dessus du chemin ne paie pas de creux.
    pub vstep: bool,
    /// Valeur d'une pièce étroite gardée en réserve, selon les puits.
    pub narrow_keep: i32,
}

/// Ajustés par la méthode Texel (`examples/tune.rs`) sur 27 000 positions
/// d'autoduel : 63,7 % puis 66,5 % contre les anciens poids (234 parties).
pub const DEFAULT_WEIGHTS: Weights = Weights {
    empty: 4,
    gap: 1,
    distance: 51,
    second_axis: 16,
    zone: 297,
    tempo: 188,
    narrow_ok: 5,
    narrow_none: 20,
    deep_wells: true,
    wells3: true,
    cliff: true,
    zone_phase: true,
    vstep: true,
    narrow_keep: 300,
};

/// Poids d'avant l'ajustement : référence des tests qui isolent une option.
pub const HAND_WEIGHTS: Weights = Weights {
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
    cliff: false,
    zone_phase: false,
    vstep: false,
    narrow_keep: 0,
};

pub const UNREACHABLE: i32 = 400;

/// Pièces étroites (mono, domino, barre) : les formes `0..NARROW_SHAPES`.
pub const NARROW_SHAPES: usize = 3;

/// Le grand L (« 4L ») dans `SHAPE_NAMES`.
const BIG_L: usize = 6;

/// Coût d'arête le plus élevé (case vide, huit creux dessous, étroite) ; la
/// file de Dial de `shortest` exige qu'il reste sous `RING`.
pub fn max_step(w: &Weights) -> i32 {
    w.empty + w.gap * (N as i32 - 1) + w.narrow_ok.max(w.narrow_none).max(0)
}

/// Pièces étroites encore en réserve pour `p`.
fn narrow_held(pos: &Position, p: usize) -> i32 {
    pos.inv[p][..NARROW_SHAPES].iter().map(|&c| c as i32).sum()
}

/// Cases vides coincées entre deux cases pleines ou un bord.
fn narrow_cells(occ: u128) -> u128 {
    !occ & (COL0 | ((occ & !COL8) << 1)) & (COL8 | ((occ & !COL0) >> 1)) & FULL
}

pub fn remaining_cells(pos: &Position, p: usize) -> i32 {
    (0..SHAPES)
        .map(|s| pos.inv[p][s] as i32 * SHAPE_SIZES[s] as i32)
        .sum()
}

/// Ce que l'évaluation lit du plateau, commun aux deux joueurs.
pub struct Surface {
    occ: u128,
    /// Case vide coincée entre deux cases pleines ou un bord : seule une pièce
    /// étroite (mono, domino, barre) peut encore s'y loger sans appui latéral.
    narrow: u128,
    /// classes[g] : cases vides avec `g` cases vides dessous ; +N si étroites.
    classes: [u128; K],
}

impl Surface {
    pub fn new(pos: &Position) -> Self {
        let occ = pos.occupied();
        let narrow = narrow_cells(occ);
        // Sans trou sous les pièces, la surface d'une colonne est sa case vide
        // la plus basse ; la classe `g` est la surface décalée de `g` rangs.
        let surface = !occ & (ROW8 | occ >> N) & FULL;
        let mut classes = [0u128; K];
        for (g, class) in classes[..N].iter_mut().enumerate() {
            *class = surface >> (g * N);
        }
        for g in 0..N {
            classes[g + N] = classes[g] & narrow;
            classes[g] &= !narrow;
        }
        Surface {
            occ,
            narrow,
            classes,
        }
    }
}

/// Zones connexes d'un joueur, calculées une fois par évaluation : `shortest`
/// y rattache une zone entière d'un coup, et la plus grande sert au départage.
pub struct Zones {
    /// Au plus une zone par pièce posée ; 48 couvre même un plateau arbitraire.
    masks: [u128; 48],
    count: usize,
    pub largest: u32,
}

impl Zones {
    pub fn new(own: u128) -> Self {
        let mut z = Zones {
            masks: [0; 48],
            count: 0,
            largest: 0,
        };
        let mut rest = own;
        while rest != 0 && z.count < z.masks.len() {
            let zone = flood(rest.isolate_lowest_one(), own);
            z.masks[z.count] = zone;
            z.count += 1;
            z.largest = z.largest.max(zone.count_ones());
            rest &= !zone;
        }
        z
    }

    fn masks(&self) -> &[u128] {
        &self.masks[..self.count]
    }
}

/// Plus court chemin d'un bord à l'opposé pour `p`, sur les deux axes.
/// Résultat : (gauche-droite, haut-bas), en unités de coût.
pub fn distances(pos: &Position, p: usize, w: &Weights) -> (i32, i32) {
    distances_on(pos, &Surface::new(pos), &Zones::new(pos.bb[p]), p, w)
}

fn distances_on(pos: &Position, s: &Surface, zones: &Zones, p: usize, w: &Weights) -> (i32, i32) {
    let own = pos.bb[p];
    let blocked = pos.bb[1 - p];
    let (occ, narrow) = (s.occ, s.narrow);
    let has_narrow = narrow_held(pos, p) > 0;
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
    if w.wells3 && pos.inv[p][BIG_L] > 0 {
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
    let costs: [i32; K] = std::array::from_fn(|k| {
        w.empty + w.gap * (k % N) as i32 + if k >= N { narrow_cost } else { 0 }
    });
    // Seules les classes non vides et non bloquées comptent dans la boucle.
    let mut active = [(0u128, 0usize); K];
    let mut n = 0;
    for (&class, &cost) in s.classes.iter().zip(&costs) {
        let cells = class & !blocked;
        if cells != 0 {
            active[n] = (cells, (cost.max(0) as usize).min(RING - 1));
            n += 1;
        }
    }
    let active = &active[..n];
    // `vstep` : la case du dessus coûte une case vide, plus le surcoût étroit.
    let to_step = |c: i32| (c.max(0) as usize).min(RING - 1);
    let vstep = w
        .vstep
        .then_some((narrow, to_step(w.empty), to_step(w.empty + narrow_cost)));
    let zones = zones.masks();
    let lr = shortest(own, zones, blocked, active, vstep, COL0, COL8);
    // Du bas vers le haut : `vstep` ne joue qu'en montant.
    let tb = shortest(own, zones, blocked, active, vstep, ROW8, ROW0);
    (lr, tb)
}

/// File de Dial circulaire : tout coût d'arête est inférieur à `RING`, donc
/// une case n'écrase jamais un seau pas encore traité.
pub const RING: usize = 64;

const K: usize = 2 * N;

fn shortest(
    own: u128,
    zones: &[u128],
    blocked: u128,
    active: &[(u128, usize)],
    vstep: Option<(u128, usize, usize)>,
    start: u128,
    goal: u128,
) -> i32 {
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
        // Les cases propres atteintes rattachent leur zone entière : deux zones
        // distinctes ne se touchent jamais, une seule passe suffit.
        let mut reach = spread(cur);
        let mut merged = false;
        if reach & own != 0 {
            for &z in zones {
                if reach & z != 0 && settled & z == 0 {
                    cur |= z;
                    merged = true;
                }
            }
        }
        if cur & goal != 0 {
            return (d as i32).min(UNREACHABLE);
        }
        if merged {
            reach = spread(cur);
        }
        settled |= cur;
        let mut frontier = reach & !settled;
        if let Some((narrow, step, narrow_step)) = vstep {
            // Posée sur une case du chemin, la case du dessus n'a plus de creux
            // à combler : une tour se compte case par case.
            let above = (cur >> N) & frontier & !own;
            push(&mut ring, &mut pending, d + step, above & !narrow);
            push(&mut ring, &mut pending, d + narrow_step, above & narrow);
            frontier &= !above;
        }
        for &(cells, step) in active {
            push(&mut ring, &mut pending, d + step, frontier & cells);
        }
    }
    UNREACHABLE
}

/// Score de connexion de `p` (plus petit = mieux), et s'il a un axe coupé.
fn connection_score(
    pos: &Position,
    s: &Surface,
    zones: &Zones,
    p: usize,
    w: &Weights,
) -> (i32, bool) {
    let (lr, tb) = distances_on(pos, s, zones, p, w);
    let (best, mut other) = if lr <= tb { (lr, tb) } else { (tb, lr) };
    let infeasible = best >= UNREACHABLE || best / w.empty.max(1) > remaining_cells(pos, p);
    // Avant tout plafonnement de `other` : un axe coupé reste coupé.
    let cut = other >= UNREACHABLE || infeasible;
    if w.cliff {
        // Pas de falaise : l'axe secondaire ne pèse jamais plus que l'écart à
        // l'axe principal, et un joueur coupé vaut la pire distance atteignable.
        other = other.min(best + 60);
    }
    let d = if !infeasible {
        best * 16 + other * w.second_axis
    } else if w.cliff {
        UNREACHABLE * 16 + (UNREACHABLE + 60) * w.second_axis
    } else {
        UNREACHABLE * 20
    };
    (d, cut)
}

/// Score pour le joueur au trait.
pub fn evaluate(pos: &Position, w: &Weights) -> i32 {
    let p = pos.to_move;
    let q = 1 - p;
    let s = Surface::new(pos);
    let (zp, zq) = (Zones::new(pos.bb[p]), Zones::new(pos.bb[q]));
    let (dp, cut_p) = connection_score(pos, &s, &zp, p, w);
    let (dq, cut_q) = connection_score(pos, &s, &zq, q, w);
    let dist = (dq - dp) * w.distance / 16;
    let filled = s.occ.count_ones() as i32;
    let zones = zp.largest as i32 - zq.largest as i32;
    let (held_p, held_q) = (narrow_held(pos, p), narrow_held(pos, q));
    let zone_term = if w.zone_phase && (cut_p || cut_q || held_p == 0 || held_q == 0) {
        // Phase de blocage : le départage aux zones devient l'enjeu.
        zones * (w.zone + 120)
    } else {
        zones * w.zone * filled / 81
    };
    let keep = if w.narrow_keep != 0 {
        // Une pièce étroite en réserve vaut d'autant plus qu'il y a de puits.
        let wells = s.narrow.count_ones() as i32;
        (held_p - held_q) * w.narrow_keep * (1 + wells.min(12)) / 4
    } else {
        0
    };
    dist + zone_term + keep + w.tempo
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::BLUE;
    use crate::notation::parse_record;

    #[test]
    fn empty_board_distances_are_symmetric() {
        let pos = Position::new(BLUE);
        let (lr, tb) = distances(&pos, BLUE, &HAND_WEIGHTS);
        assert_eq!(lr, 9 * HAND_WEIGHTS.empty);
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
                ..HAND_WEIGHTS
            },
        );
        let aware = distances(
            &pos,
            BLUE,
            &Weights {
                deep_wells: true,
                ..HAND_WEIGHTS
            },
        );
        assert!(naive.0 < UNREACHABLE && naive.1 < UNREACHABLE);
        assert_eq!(aware, (UNREACHABLE, UNREACHABLE));
    }

    #[test]
    fn vertical_step_makes_towers_as_cheap_as_rows() {
        let pos = Position::new(BLUE);
        let w = Weights {
            vstep: true,
            ..HAND_WEIGHTS
        };
        let (lr, tb) = distances(&pos, BLUE, &w);
        assert_eq!(lr, 9 * w.empty);
        assert_eq!(tb, 9 * w.empty);
    }

    #[test]
    fn cliff_caps_the_second_axis_and_keeps_the_cut_flag() {
        // Bleu tient le fond de la colonne 1 à la colonne 6 : son axe gauche-droite
        // est court, l'axe haut-bas lointain.
        let pos = parse_record("3I1 11 3I4 12").unwrap();
        let surface = Surface::new(&pos);
        let zones = Zones::new(pos.bb[BLUE]);
        let plain = connection_score(&pos, &surface, &zones, BLUE, &HAND_WEIGHTS);
        let capped = connection_score(
            &pos,
            &surface,
            &zones,
            BLUE,
            &Weights {
                cliff: true,
                ..HAND_WEIGHTS
            },
        );
        assert!(capped.0 <= plain.0);
        let (lr, tb) = distances(&pos, BLUE, &HAND_WEIGHTS);
        let (best, other) = (lr.min(tb), lr.max(tb));
        assert_eq!(
            capped.0,
            best * 16 + other.min(best + 60) * HAND_WEIGHTS.second_axis
        );
        assert_eq!(capped.1, plain.1);
    }

    #[test]
    fn keeping_narrow_pieces_is_rewarded() {
        let mut pos = parse_record("4Lr35 4Lr32").unwrap();
        let w = Weights {
            narrow_keep: 40,
            ..HAND_WEIGHTS
        };
        let before = evaluate(&pos, &w) - evaluate(&pos, &HAND_WEIGHTS);
        assert_eq!(before, 0);
        pos.inv[1 - pos.to_move][0] = 0;
        pos.inv[1 - pos.to_move][1] = 0;
        assert!(evaluate(&pos, &w) - evaluate(&pos, &HAND_WEIGHTS) > 0);
    }

    #[test]
    fn default_weights_are_usable() {
        // Les tests ci-dessus isolent une option sur `HAND_WEIGHTS` : ceux-ci
        // couvrent les poids réellement livrés.
        assert!((max_step(&DEFAULT_WEIGHTS) as usize) < RING);
        let pos = Position::new(BLUE);
        let (lr, tb) = distances(&pos, BLUE, &DEFAULT_WEIGHTS);
        assert_eq!(lr, 9 * DEFAULT_WEIGHTS.empty);
        // `vstep` : une tour coûte autant qu'une rangée.
        assert_eq!(tb, lr);
    }

    #[test]
    fn own_floor_row_shortens_distance() {
        let pos = parse_record("3I1 11 3I4").unwrap();
        let (lr, _) = distances(&pos, BLUE, &HAND_WEIGHTS);
        assert_eq!(lr, 3 * HAND_WEIGHTS.empty);
    }
}
