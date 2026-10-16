//! Recherche alpha-bêta à approfondissement itératif, bornée dans le temps.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::eval::{DEFAULT_WEIGHTS, Weights, evaluate};
use crate::game::{Move, Position, drop_mask, flood, spans, spread};
use crate::pieces::{SHAPES, orientations};

pub const MATE: i32 = 1_000_000;
const MAX_PLY: usize = 64;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub best: Move,
    pub score: i32,
    pub depth: u32,
    pub nodes: u64,
    pub elapsed: Duration,
}

#[derive(Clone, Copy, Default)]
struct Entry {
    key: u64,
    depth: i8,
    bound: u8,
    score: i32,
    best: Option<Move>,
}

const EXACT: u8 = 0;
const LOWER: u8 = 1;
const UPPER: u8 = 2;

struct Zobrist {
    cells: [[u64; 81]; 2],
    inv: [[[u64; 3]; SHAPES]; 2],
    side: u64,
}

fn zobrist() -> &'static Zobrist {
    static Z: OnceLock<Zobrist> = OnceLock::new();
    Z.get_or_init(|| {
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut z = Zobrist { cells: [[0; 81]; 2], inv: [[[0; 3]; SHAPES]; 2], side: 0 };
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

pub fn hash(pos: &Position) -> u64 {
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

pub struct Searcher {
    tt: Vec<Entry>,
    mask: usize,
    history: Box<[[[u32; 9]; 8]; SHAPES]>,
    killers: [[Option<Move>; 2]; MAX_PLY],
    nodes: u64,
    deadline: Instant,
    stopped: bool,
    pub weights: Weights,
}

impl Searcher {
    pub fn new(tt_bits: u32) -> Self {
        let size = 1usize << tt_bits;
        Searcher {
            tt: vec![Entry::default(); size],
            mask: size - 1,
            history: Box::new([[[0; 9]; 8]; SHAPES]),
            killers: [[None; 2]; MAX_PLY],
            nodes: 0,
            deadline: Instant::now(),
            stopped: false,
            weights: DEFAULT_WEIGHTS,
        }
    }

    fn time_up(&mut self) -> bool {
        if self.nodes & 255 == 0 && Instant::now() >= self.deadline {
            self.stopped = true;
        }
        self.stopped
    }

    /// Coup gagnant immédiat du joueur au trait, s'il existe.
    fn winning_move(pos: &Position, moves: &[Move]) -> Option<Move> {
        let own = pos.bb[pos.to_move];
        let mut zones = [0u128; 41];
        let mut count = 0;
        let mut rest = own;
        while rest != 0 {
            let z = flood(rest & rest.wrapping_neg(), own);
            zones[count] = z;
            count += 1;
            rest &= !z;
        }
        moves.iter().copied().find(|&m| {
            let added = drop_mask(&pos.heights, m).unwrap();
            let touching = spread(added);
            let mut merged = added;
            for &z in &zones[..count] {
                if z & touching != 0 {
                    merged |= z;
                }
            }
            spans(merged)
        })
    }

    fn order(&self, moves: &mut [Move], tt_move: Option<Move>, ply: usize) {
        let k = self.killers[ply.min(MAX_PLY - 1)];
        moves.sort_by_cached_key(|m| {
            let mut s = self.history[m.shape as usize][m.orient as usize][m.col as usize] as i64;
            if Some(*m) == tt_move {
                s += 1 << 40;
            } else if Some(*m) == k[0] || Some(*m) == k[1] {
                s += 1 << 30;
            }
            s += orientations(m.shape as usize)[m.orient as usize].cells.len() as i64;
            -s
        });
    }

    fn terminal_score(pos: &Position, ply: usize) -> i32 {
        match pos.winner() {
            Some(w) if w == pos.to_move => MATE - ply as i32,
            Some(_) => -(MATE - ply as i32),
            None => 0,
        }
    }

    fn negamax(&mut self, pos: &Position, depth: i32, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.nodes += 1;
        if pos.outcome.is_some() {
            return Self::terminal_score(pos, ply);
        }
        if self.time_up() {
            return 0;
        }
        let mut moves = Vec::with_capacity(160);
        pos.legal_moves(&mut moves);
        if Self::winning_move(pos, &moves).is_some() {
            return MATE - ply as i32 - 1;
        }
        if depth <= 0 || ply >= MAX_PLY - 1 {
            return evaluate(pos, &self.weights);
        }

        let key = hash(pos);
        let slot = (key as usize) & self.mask;
        let e = self.tt[slot];
        let mut tt_move = None;
        if e.key == key {
            tt_move = e.best;
            if e.depth as i32 >= depth {
                let s = e.score;
                if e.bound == EXACT
                    || (e.bound == LOWER && s >= beta)
                    || (e.bound == UPPER && s <= alpha)
                {
                    return s;
                }
            }
        }

        self.order(&mut moves, tt_move, ply);
        let alpha0 = alpha;
        let mut best = -MATE * 2;
        let mut best_move = moves[0];
        for (i, &m) in moves.iter().enumerate() {
            let mut child = *pos;
            child.play(m).unwrap();
            let same = child.to_move == pos.to_move;
            let search = |s: &mut Self, a: i32, b: i32| {
                if same {
                    s.negamax(&child, depth - 1, a, b, ply + 1)
                } else {
                    -s.negamax(&child, depth - 1, -b, -a, ply + 1)
                }
            };
            let mut score;
            if i == 0 {
                score = search(self, alpha, beta);
            } else {
                score = search(self, alpha, alpha + 1);
                if score > alpha && score < beta && !self.stopped {
                    score = search(self, alpha, beta);
                }
            }
            if self.stopped {
                return 0;
            }
            if score > best {
                best = score;
                best_move = m;
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                let k = &mut self.killers[ply.min(MAX_PLY - 1)];
                if k[0] != Some(m) {
                    k[1] = k[0];
                    k[0] = Some(m);
                }
                self.history[m.shape as usize][m.orient as usize][m.col as usize] +=
                    (depth * depth) as u32;
                break;
            }
        }
        let bound = if best <= alpha0 {
            UPPER
        } else if best >= beta {
            LOWER
        } else {
            EXACT
        };
        self.tt[slot] = Entry { key, depth: depth as i8, bound, score: best, best: Some(best_move) };
        best
    }

    pub fn search(&mut self, pos: &Position, budget: Duration, max_depth: u32) -> Option<SearchResult> {
        let start = Instant::now();
        self.deadline = start + budget;
        self.stopped = false;
        self.nodes = 0;
        let mut moves = Vec::new();
        pos.legal_moves(&mut moves);
        if moves.is_empty() || pos.outcome.is_some() {
            return None;
        }
        let done = |best, score, depth, nodes| SearchResult {
            best,
            score,
            depth,
            nodes,
            elapsed: start.elapsed(),
        };
        if let Some(m) = Self::winning_move(pos, &moves) {
            return Some(done(m, MATE - 1, 1, 1));
        }
        self.order(&mut moves, None, 0);
        let mut result = done(moves[0], 0, 0, 0);
        for depth in 1..=max_depth {
            let mut alpha = -MATE * 2;
            let beta = MATE * 2;
            let mut iter_best: Option<(Move, i32)> = None;
            let mut scored: Vec<(Move, i32)> = Vec::with_capacity(moves.len());
            for (i, &m) in moves.iter().enumerate() {
                let mut child = *pos;
                child.play(m).unwrap();
                let same = child.to_move == pos.to_move;
                let run = |s: &mut Self, a: i32, b: i32| {
                    if same {
                        s.negamax(&child, depth as i32 - 1, a, b, 1)
                    } else {
                        -s.negamax(&child, depth as i32 - 1, -b, -a, 1)
                    }
                };
                let mut score = if i == 0 { run(self, alpha, beta) } else { run(self, alpha, alpha + 1) };
                if i > 0 && score > alpha && !self.stopped {
                    score = run(self, alpha, beta);
                }
                if self.stopped {
                    break;
                }
                scored.push((m, score));
                if score > alpha {
                    alpha = score;
                    iter_best = Some((m, score));
                }
            }
            if let Some((m, s)) = iter_best {
                // Un meilleur coup trouvé dans une itération interrompue a été
                // entièrement vérifié : on le garde.
                result = done(m, s, if self.stopped { depth - 1 } else { depth }, self.nodes);
            }
            if self.stopped {
                break;
            }
            // Réordonner la racine : meilleurs scores d'abord, le reste derrière.
            scored.sort_by_key(|&(_, s)| -s);
            moves = scored.into_iter().map(|(m, _)| m).collect();
            if alpha.abs() >= MATE - MAX_PLY as i32 {
                break;
            }
            if start.elapsed() * 3 > budget {
                break;
            }
        }
        result.nodes = self.nodes;
        result.elapsed = start.elapsed();
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::{format_move, parse_record};

    #[test]
    fn finds_immediate_win() {
        let pos = parse_record("3I1 11 3I4 12").unwrap();
        let r = Searcher::new(16).search(&pos, Duration::from_millis(500), 4).unwrap();
        let mut after = pos;
        after.play(r.best).unwrap();
        assert_eq!(after.winner(), Some(0), "{}", format_move(r.best));
    }

    #[test]
    fn blocks_opponent_immediate_win() {
        // Blanc tient le fond des colonnes 1 à 6 et menace 4L7.
        let pos = parse_record("18 3I1 19 3I4").unwrap();
        let mut s = Searcher::new(18);
        let r = s.search(&pos, Duration::from_millis(1500), 3).unwrap();
        let mut after = pos;
        after.play(r.best).unwrap();
        let mut replies = Vec::new();
        after.legal_moves(&mut replies);
        assert!(Searcher::winning_move(&after, &replies).is_none(), "{}", format_move(r.best));
    }

    #[test]
    fn always_returns_a_legal_move_from_start() {
        let pos = Position::new(0);
        let r = Searcher::new(16).search(&pos, Duration::from_millis(200), 2).unwrap();
        let mut p = pos;
        assert!(p.play(r.best).is_ok());
    }
}
