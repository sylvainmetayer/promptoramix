//! Position, coups légaux, chute, connexion et fin de partie.

use std::sync::OnceLock;

use crate::pieces::{SHAPE_SIZES, SHAPES, orientations};

pub const N: usize = 9;
pub const FULL: u128 = (1u128 << 81) - 1;

const fn col_mask(x: usize) -> u128 {
    let mut m = 0u128;
    let mut y = 0;
    while y < N {
        m |= 1u128 << (y * N + x);
        y += 1;
    }
    m
}
pub const COL0: u128 = col_mask(0);
pub const COL8: u128 = col_mask(8);
pub const ROW0: u128 = (1u128 << 9) - 1;
pub const ROW8: u128 = ROW0 << 72;

pub const BLUE: usize = 0;
pub const WHITE: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Syntax,
    Exhausted,
    HorizontalBounds,
    Overflow,
    Unsupported,
    Finished,
    UnexpectedPass,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Syntax => "syntax",
            Refusal::Exhausted => "exhausted",
            Refusal::HorizontalBounds => "horizontal-bounds",
            Refusal::Overflow => "overflow",
            Refusal::Unsupported => "unsupported",
            Refusal::Finished => "finished",
            Refusal::UnexpectedPass => "unexpected-pass",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Move {
    pub shape: u8,
    pub orient: u8,
    pub col: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Connection(usize),
    Blocked {
        winner: Option<usize>,
        scores: [u32; 2],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub bb: [u128; 2],
    /// Empreinte Zobrist de la position (`full_hash`), mise à jour à chaque pose.
    pub key: u64,
    pub heights: [u8; N],
    pub inv: [[u8; SHAPES]; 2],
    pub to_move: usize,
    pub outcome: Option<Outcome>,
}

/// Cases ajoutées par un coup, ou motif de refus.
pub fn drop_mask(heights: &[u8; N], m: Move) -> Result<u128, Refusal> {
    let o = &orientations(m.shape as usize)[m.orient as usize];
    let col = m.col as usize;
    if col + o.width > N {
        return Err(Refusal::HorizontalBounds);
    }
    let mut anchor = i32::MAX;
    for c in 0..o.width {
        let free_row = 8 - heights[col + c] as i32;
        anchor = anchor.min(free_row - o.col_bottom[c] as i32);
    }
    if anchor < 0 {
        return Err(Refusal::Overflow);
    }
    for c in 0..o.width {
        if anchor + o.col_bottom[c] as i32 != 8 - heights[col + c] as i32 {
            return Err(Refusal::Unsupported);
        }
    }
    Ok((o.mask << col) << (anchor as usize * N))
}

/// Étend un ensemble de cases à ses huit voisines.
#[inline]
pub fn spread(s: u128) -> u128 {
    let r = s & !COL8;
    let l = s & !COL0;
    (s | s << 9 | s >> 9 | r << 1 | r << 10 | r >> 8 | l >> 1 | l >> 10 | l << 8) & FULL
}

/// Zone connexe de `own` contenant `seed`.
#[inline]
pub fn flood(seed: u128, own: u128) -> u128 {
    let mut zone = seed & own;
    loop {
        let next = spread(zone) & own;
        if next == zone {
            return zone;
        }
        zone = next;
    }
}

pub fn spans(zone: u128) -> bool {
    (zone & COL0 != 0 && zone & COL8 != 0) || (zone & ROW0 != 0 && zone & ROW8 != 0)
}

/// Faux seulement si `p` ne peut gagner d'aucun coup : même en posant les
/// cases d'une seule pièce n'importe où à trois rangs ou moins au-dessus de la
/// surface, aucun bord ne rejoint l'opposé.
pub fn may_win_now(pos: &Position, p: usize) -> bool {
    // Une pièce tient sur trois colonnes et trois rangs : en connexion à huit
    // voisins, trois de ses cases (une par colonne, ou par rang) suffisent.
    let Some(cells) = (0..SHAPES)
        .filter(|&s| pos.inv[p][s] > 0)
        .map(|s| SHAPE_SIZES[s].min(3))
        .max()
    else {
        return false;
    };
    let own = pos.bb[p];
    let occ = pos.occupied();
    // Sans trou sous les pièces, la surface est la case vide au-dessus d'une
    // pleine ; une pièce n'a pas plus de trois cases de haut.
    let surface = !occ & (ROW8 | occ >> N) & FULL;
    let reach = surface | surface >> N | surface >> (2 * N);
    let joins = |start: u128, goal: u128| {
        // `zone` : ce que relient `k` cases posées au plus.
        let mut zone = flood(start, own);
        for _ in 0..cells {
            if zone & goal != 0 {
                return true;
            }
            let next = zone | ((spread(zone) | start) & reach);
            zone = flood(next, own | next);
        }
        zone & goal != 0
    };
    joins(COL0, COL8) || joins(ROW8, ROW0)
}

pub fn largest_zone(own: u128) -> u32 {
    let mut rest = own;
    let mut best = 0;
    while rest != 0 {
        let zone = flood(rest.isolate_lowest_one(), own);
        best = best.max(zone.count_ones());
        rest &= !zone;
    }
    best
}

pub(crate) struct Zobrist {
    cells: [[u64; 81]; 2],
    inv: [[[u64; 3]; SHAPES]; 2],
    side: u64,
}

pub(crate) fn zobrist() -> &'static Zobrist {
    static Z: OnceLock<Zobrist> = OnceLock::new();
    Z.get_or_init(|| {
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut z = Zobrist {
            cells: [[0; 81]; 2],
            inv: [[[0; 3]; SHAPES]; 2],
            side: 0,
        };
        for p in 0..2 {
            for i in 0..81 {
                z.cells[p][i] = next();
            }
            for sh in 0..SHAPES {
                for c in 0..3 {
                    z.inv[p][sh][c] = next();
                }
            }
        }
        z.side = next();
        z
    })
}

/// Empreinte calculée de zéro ; `Position::key` la tient à jour.
pub fn full_hash(pos: &Position) -> u64 {
    let z = zobrist();
    let mut h = if pos.to_move == 1 { z.side } else { 0 };
    for p in 0..2 {
        let mut b = pos.bb[p];
        while b != 0 {
            h ^= z.cells[p][b.trailing_zeros() as usize];
            b &= b - 1;
        }
        for sh in 0..SHAPES {
            h ^= z.inv[p][sh][pos.inv[p][sh] as usize];
        }
    }
    h
}

impl Position {
    pub fn new(first: usize) -> Self {
        let mut pos = Position {
            bb: [0, 0],
            key: 0,
            heights: [0; N],
            inv: [[2; SHAPES]; 2],
            to_move: first,
            outcome: None,
        };
        pos.key = full_hash(&pos);
        pos
    }

    pub fn occupied(&self) -> u128 {
        self.bb[0] | self.bb[1]
    }

    /// Appelle `f` pour chaque coup légal de `player` et ses cases ajoutées,
    /// jusqu'à ce que `f` renvoie `true` ; renvoie alors `true`.
    fn any_drop(&self, player: usize, mut f: impl FnMut(Move, u128) -> bool) -> bool {
        for shape in 0..SHAPES {
            if self.inv[player][shape] == 0 {
                continue;
            }
            for (oi, o) in orientations(shape).iter().enumerate() {
                for col in 0..=(N - o.width) {
                    let m = Move {
                        shape: shape as u8,
                        orient: oi as u8,
                        col: col as u8,
                    };
                    if let Ok(added) = drop_mask(&self.heights, m)
                        && f(m, added)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn legal_moves_for(&self, player: usize, out: &mut Vec<Move>) {
        out.clear();
        self.any_drop(player, |m, _| {
            out.push(m);
            false
        });
    }

    pub fn legal_moves(&self, out: &mut Vec<Move>) {
        self.legal_moves_for(self.to_move, out)
    }

    /// Coups légaux avec leurs cases ajoutées, pour `play_drop`.
    pub fn legal_drops_for(&self, player: usize, out: &mut Vec<(Move, u128)>) {
        out.clear();
        self.any_drop(player, |m, added| {
            out.push((m, added));
            false
        });
    }

    pub fn has_move(&self, player: usize) -> bool {
        self.any_drop(player, |_, _| true)
    }

    /// Joue un coup du joueur au trait, puis applique victoire, passe ou blocage.
    pub fn play(&mut self, m: Move) -> Result<(), Refusal> {
        if self.outcome.is_some() {
            return Err(Refusal::Finished);
        }
        let p = self.to_move;
        if self.inv[p][m.shape as usize] == 0 {
            return Err(Refusal::Exhausted);
        }
        let added = drop_mask(&self.heights, m)?;
        self.play_drop(m, added);
        Ok(())
    }

    /// Joue sans contrôle un coup de `legal_drops_for` de la position courante.
    #[inline]
    pub fn play_drop(&mut self, m: Move, added: u128) {
        let p = self.to_move;
        self.place(p, m.shape as usize, added);
        self.advance(p, added);
    }

    /// Pose sans contrôle : `added` vient de `drop_mask`.
    #[inline]
    pub fn place(&mut self, p: usize, shape: usize, added: u128) {
        let z = zobrist();
        let count = self.inv[p][shape] as usize;
        self.key ^= z.inv[p][shape][count] ^ z.inv[p][shape][count - 1];
        let mut cells = added;
        while cells != 0 {
            self.key ^= z.cells[p][cells.trailing_zeros() as usize];
            cells &= cells - 1;
        }
        self.inv[p][shape] -= 1;
        self.bb[p] |= added;
        let occ = self.occupied();
        let mut cols = added;
        for x in 0..N {
            if cols & col_mask(x) != 0 {
                let filled = (occ & col_mask(x)).count_ones();
                self.heights[x] = filled as u8;
                cols &= !col_mask(x);
            }
        }
    }

    fn set_to_move(&mut self, player: usize) {
        if self.to_move != player {
            self.key ^= zobrist().side;
            self.to_move = player;
        }
    }

    pub fn advance(&mut self, p: usize, added: u128) {
        let q = 1 - p;
        if spans(flood(added, self.bb[p])) {
            self.outcome = Some(Outcome::Connection(p));
        } else if self.has_move(q) {
            self.set_to_move(q);
        } else if self.has_move(p) {
            self.set_to_move(p);
        } else {
            let scores = [largest_zone(self.bb[0]), largest_zone(self.bb[1])];
            let winner = match scores[0].cmp(&scores[1]) {
                std::cmp::Ordering::Greater => Some(BLUE),
                std::cmp::Ordering::Less => Some(WHITE),
                std::cmp::Ordering::Equal => None,
            };
            self.outcome = Some(Outcome::Blocked { winner, scores });
        }
    }

    pub fn winner(&self) -> Option<usize> {
        match self.outcome {
            Some(Outcome::Connection(p)) => Some(p),
            Some(Outcome::Blocked { winner, .. }) => winner,
            None => None,
        }
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for y in 0..N {
            for x in 0..N {
                let bit = 1u128 << (y * N + x);
                s.push(if self.bb[0] & bit != 0 {
                    'B'
                } else if self.bb[1] & bit != 0 {
                    'W'
                } else {
                    '.'
                });
            }
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pieces::orientation_index;

    fn mv(shape: usize, rot: u8, flip: bool, col: u8) -> Move {
        Move {
            shape: shape as u8,
            orient: orientation_index(shape, rot, flip) as u8,
            col,
        }
    }

    #[test]
    fn t_stem_down_on_flat_floor_is_unsupported() {
        let p = Position::new(BLUE);
        assert_eq!(
            drop_mask(&p.heights, mv(5, 0, false, 0))
                .unwrap_err()
                .code(),
            "unsupported"
        );
        assert!(drop_mask(&p.heights, mv(5, 2, false, 0)).is_ok());
        let t2 = &orientations(5)[orientation_index(5, 2, false)];
        assert_eq!(t2.cells, vec![(1, 0), (0, 1), (1, 1), (2, 1)]);
    }

    #[test]
    fn t_stem_down_over_two_supports() {
        let mut p = Position::new(BLUE);
        p.play(mv(0, 0, false, 0)).unwrap();
        p.play(mv(0, 0, false, 2)).unwrap();
        assert!(drop_mask(&p.heights, mv(5, 0, false, 0)).is_ok());
        let added = drop_mask(&p.heights, mv(5, 0, false, 0)).unwrap();
        assert_eq!(added.count_ones(), 4);
        assert_eq!(added & ROW8, 1 << 73);
    }

    #[test]
    fn horizontal_bounds_and_overflow() {
        let mut p = Position::new(BLUE);
        assert_eq!(
            drop_mask(&p.heights, mv(2, 0, false, 7)),
            Err(Refusal::HorizontalBounds)
        );
        p.heights[4] = 8;
        assert_eq!(
            drop_mask(&p.heights, mv(1, 1, false, 4)),
            Err(Refusal::Overflow)
        );
        p.heights[4] = 7;
        assert!(drop_mask(&p.heights, mv(1, 1, false, 4)).is_ok());
    }

    #[test]
    fn diagonal_connection_spans() {
        let mut own = 0u128;
        for i in 0..N {
            own |= 1u128 << (i * N + i);
        }
        assert!(spans(flood(1, own)));
        assert_eq!(largest_zone(own), 9);
        assert_eq!(flood(1, own & !(1u128 << 40)).count_ones(), 4);
    }

    #[test]
    fn spread_does_not_wrap_rows() {
        assert_eq!(spread(1u128 << 8) & (1u128 << 9), 0);
        assert_eq!(spread(1u128 << 9) & (1u128 << 8), 0);
    }

    #[test]
    fn incremental_key_matches_full_hash() {
        let mut seed = 0x1234_5678_9ABC_DEF1u64;
        let mut moves = Vec::new();
        for first in [BLUE, WHITE] {
            for _ in 0..40 {
                let mut p = Position::new(first);
                while p.outcome.is_none() {
                    assert_eq!(p.key, full_hash(&p));
                    p.legal_moves(&mut moves);
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    p.play(moves[(seed % moves.len() as u64) as usize]).unwrap();
                }
                assert_eq!(p.key, full_hash(&p));
            }
        }
    }

    #[test]
    fn refusal_changes_nothing() {
        let mut p = Position::new(BLUE);
        let before = p;
        assert!(p.play(mv(5, 0, false, 0)).is_err());
        assert_eq!(p, before);
    }
}
