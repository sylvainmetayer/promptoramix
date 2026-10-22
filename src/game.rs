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

/// Case vide la plus basse de chaque colonne (aucune pour une colonne
/// pleine) : sans trou sous les pièces, c'est là que se pose le prochain coup.
#[inline]
pub fn surface(occ: u128) -> u128 {
    !occ & (ROW8 | occ >> N) & FULL
}

/// Cases renvoyées par un miroir gauche-droite : la colonne `x` passe en `8 - x`.
pub fn mirror_cells(cells: u128) -> u128 {
    let mut out = 0u128;
    for x in 0..N {
        let column = (cells >> x) & COL0;
        out |= column << (N - 1 - x);
    }
    out
}

/// Le coup qui, joué sur la position miroir, pose les cases miroirs.
pub fn mirror_move(m: Move) -> Move {
    let shape = m.shape as usize;
    let o = &orientations(shape)[m.orient as usize];
    let w = o.width as i32;
    let cells = crate::pieces::normalize(
        &o.cells
            .iter()
            .map(|&(x, y)| (w - 1 - x, y))
            .collect::<Vec<_>>(),
    );
    let orient = orientations(shape)
        .iter()
        .position(|c| c.cells == cells)
        .expect("toute orientation a son miroir parmi celles de sa forme");
    Move {
        shape: m.shape,
        orient: orient as u8,
        col: (N - o.width) as u8 - m.col,
    }
}

/// Étend un ensemble de cases à ses huit voisines.
#[inline]
pub fn spread(s: u128) -> u128 {
    // Voisins latéraux d'abord, puis la rangée du dessus et du dessous de ce
    // trait de trois : quatre décalages au lieu de huit.
    let h = s | (s & !COL8) << 1 | (s & !COL0) >> 1;
    (h | h << 9 | h >> 9) & FULL
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
    // Une pièce n'a pas plus de trois cases de haut.
    let surface = surface(occ);
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

    /// La même position vue dans un miroir gauche-droite, seule symétrie du
    /// jeu : la gravité interdit le haut-bas, et les réserves restent celles
    /// de chaque joueur.
    pub fn mirrored(&self) -> Self {
        let mut m = *self;
        m.bb = [mirror_cells(self.bb[0]), mirror_cells(self.bb[1])];
        m.heights.reverse();
        m.key = full_hash(&m);
        m
    }

    /// Appelle `f` pour chaque coup légal de `player` et ses cases ajoutées,
    /// jusqu'à ce que `f` renvoie `true` ; renvoie alors `true`.
    fn any_drop(&self, player: usize, mut f: impl FnMut(Move, u128) -> bool) -> bool {
        // Une pose tient si chaque colonne de la pièce repose sur la surface :
        // le dénivelé entre colonnes voisines doit être exactement celui de son
        // profil (`dd`), et l'ancrage doit rester dans le plateau. Le dénivelé
        // du plateau est calculé une fois, puis chaque coup se teste en deux
        // comparaisons au lieu de rejouer `drop_mask`.
        let h = &self.heights;
        let mut d1 = [0i8; N];
        let mut d2 = [0i8; N];
        for x in 0..N - 1 {
            d1[x] = h[x + 1] as i8 - h[x] as i8;
        }
        for x in 0..N - 2 {
            d2[x] = h[x + 2] as i8 - h[x] as i8;
        }
        for shape in 0..SHAPES {
            if self.inv[player][shape] == 0 {
                continue;
            }
            for (oi, o) in orientations(shape).iter().enumerate() {
                for col in 0..=(N - o.width) {
                    let mismatch =
                        ((d1[col] ^ o.dd[0]) & o.dm[0]) | ((d2[col] ^ o.dd[1]) & o.dm[1]);
                    if mismatch != 0 {
                        continue;
                    }
                    let anchor = 8 - h[col] as i32 - o.col_bottom[0] as i32;
                    if anchor < 0 {
                        continue;
                    }
                    let added = (o.mask << col) << (anchor as usize * N);
                    let m = Move {
                        shape: shape as u8,
                        orient: oi as u8,
                        col: col as u8,
                    };
                    if f(m, added) {
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
    fn fast_generation_matches_drop_mask() {
        // `any_drop` teste la pose par dénivelés ; `drop_mask` reste la référence.
        let mut seed = 0x5EED_1234_ABCD_0001u64;
        let mut moves = Vec::new();
        for game in 0..60 {
            let mut p = Position::new(game % 2);
            while p.outcome.is_none() {
                for player in 0..2 {
                    p.legal_drops_for(player, &mut moves);
                    let mut expected = Vec::new();
                    for shape in 0..SHAPES {
                        if p.inv[player][shape] == 0 {
                            continue;
                        }
                        for oi in 0..orientations(shape).len() {
                            for col in 0..N as u8 {
                                let m = Move {
                                    shape: shape as u8,
                                    orient: oi as u8,
                                    col,
                                };
                                if let Ok(added) = drop_mask(&p.heights, m) {
                                    expected.push((m, added));
                                }
                            }
                        }
                    }
                    assert_eq!(moves, expected, "{}", p.render());
                }
                p.legal_drops_for(p.to_move, &mut moves);
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let (m, added) = moves[(seed % moves.len() as u64) as usize];
                p.play_drop(m, added);
            }
        }
    }

    #[test]
    fn mirror_commutes_with_play() {
        // Jouer un coup puis tout refléter, ou refléter puis jouer le coup
        // miroir, mène à la même position : même empreinte, même issue.
        let mut seed = 0xFACE_0FF1_CE00_0001u64;
        let mut moves = Vec::new();
        for game in 0..60 {
            let mut p = Position::new(game % 2);
            assert_eq!(p.mirrored(), p);
            while p.outcome.is_none() {
                p.legal_moves(&mut moves);
                let mut mirrored = Vec::new();
                p.mirrored().legal_moves(&mut mirrored);
                let mut expected: Vec<Move> = moves.iter().map(|&m| mirror_move(m)).collect();
                expected.sort_by_key(|m| (m.shape, m.orient, m.col));
                mirrored.sort_by_key(|m| (m.shape, m.orient, m.col));
                assert_eq!(mirrored, expected, "{}", p.render());
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let m = moves[(seed % moves.len() as u64) as usize];
                assert_eq!(mirror_move(mirror_move(m)), m);
                let mut reflected = p.mirrored();
                reflected.play(mirror_move(m)).unwrap();
                p.play(m).unwrap();
                assert_eq!(reflected, p.mirrored(), "{}", p.render());
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
