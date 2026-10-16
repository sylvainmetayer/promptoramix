//! Recherche alpha-bêta à approfondissement itératif, bornée dans le temps.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, OnceLock};
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

#[derive(Clone, Copy)]
struct Entry {
    depth: i8,
    bound: u8,
    score: i32,
    best: Option<Move>,
}

impl Entry {
    fn pack(self) -> u64 {
        let mv = self.best.map_or(0, |m| {
            1 | (m.shape as u64) << 1 | (m.orient as u64) << 4 | (m.col as u64) << 7
        });
        (self.score as u32 as u64)
            | (self.depth as u8 as u64) << 32
            | (self.bound as u64) << 40
            | mv << 42
    }

    fn unpack(data: u64) -> Self {
        let mv = data >> 42;
        Entry {
            score: data as u32 as i32,
            depth: (data >> 32) as u8 as i8,
            bound: ((data >> 40) & 3) as u8,
            best: (mv & 1 == 1).then_some(Move {
                shape: ((mv >> 1) & 7) as u8,
                orient: ((mv >> 4) & 7) as u8,
                col: ((mv >> 7) & 15) as u8,
            }),
        }
    }
}

/// Table de transposition partagée entre fils, sans verrou : la clé est
/// stockée XOR la donnée, une écriture déchirée se lit comme une absence.
pub struct Table {
    slots: Vec<[AtomicU64; 2]>,
    mask: usize,
}

impl Table {
    pub fn new(bits: u32) -> Self {
        let size = 1usize << bits;
        Table {
            slots: (0..size)
                .map(|_| [AtomicU64::new(0), AtomicU64::new(0)])
                .collect(),
            mask: size - 1,
        }
    }

    fn probe(&self, key: u64) -> Option<Entry> {
        let slot = &self.slots[key as usize & self.mask];
        let (k, d) = (slot[0].load(Relaxed), slot[1].load(Relaxed));
        (k ^ d == key && d != 0).then(|| Entry::unpack(d))
    }

    fn store(&self, key: u64, e: Entry) {
        let slot = &self.slots[key as usize & self.mask];
        let d = e.pack();
        slot[0].store(key ^ d, Relaxed);
        slot[1].store(d, Relaxed);
    }
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
    tt: Arc<Table>,
    stop: Arc<AtomicBool>,
    helper: bool,
    history: Box<[[[u32; 9]; 8]; SHAPES]>,
    killers: [[Option<Move>; 2]; MAX_PLY],
    nodes: u64,
    deadline: Instant,
    stopped: bool,
    pub weights: Weights,
    pub options: Options,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub lmr: bool,
    pub eval_order: bool,
    /// Ne pas entamer d'itération après le premier tiers du budget.
    pub stop_early: bool,
    /// Nombre de fils de recherche (Lazy SMP), 1 par défaut.
    pub threads: usize,
}

pub const DEFAULT_OPTIONS: Options = Options {
    lmr: true,
    eval_order: true,
    stop_early: false,
    threads: 1,
};

impl Searcher {
    pub fn new(tt_bits: u32) -> Self {
        Searcher {
            tt: Arc::new(Table::new(tt_bits)),
            stop: Arc::new(AtomicBool::new(false)),
            helper: false,
            history: Box::new([[[0; 9]; 8]; SHAPES]),
            killers: [[None; 2]; MAX_PLY],
            nodes: 0,
            deadline: Instant::now(),
            stopped: false,
            weights: DEFAULT_WEIGHTS,
            options: DEFAULT_OPTIONS,
        }
    }

    /// Réglages `clé=valeur,…` (poids d'évaluation et options), pour les bancs d'essai.
    pub fn configure(&mut self, spec: &str) {
        let s = self;
        for kv in spec.split(',').filter(|k| !k.is_empty()) {
            let (k, v) = kv.split_once('=').expect("clé=valeur");
            let n: i32 = v.parse().unwrap();
            let w = &mut s.weights;
            match k {
                "empty" => w.empty = n,
                "gap" => w.gap = n,
                "distance" => w.distance = n,
                "second" => w.second_axis = n,
                "zone" => w.zone = n,
                "tempo" => w.tempo = n,
                "nok" => w.narrow_ok = n,
                "nnone" => w.narrow_none = n,
                "deep" => w.deep_wells = n != 0,
                "lmr" => s.options.lmr = n != 0,
                "eo" => s.options.eval_order = n != 0,
                "early" => s.options.stop_early = n != 0,
                "threads" => s.options.threads = n.max(1) as usize,
                _ => panic!("clé inconnue {k}"),
            }
        }
    }

    fn time_up(&mut self) -> bool {
        if self.nodes & 255 == 0 {
            if !self.helper && Instant::now() >= self.deadline {
                self.stop.store(true, Relaxed);
            }
            if self.stop.load(Relaxed) {
                self.stopped = true;
            }
        }
        self.stopped
    }

    fn spawn_helper(&self) -> Searcher {
        Searcher {
            tt: self.tt.clone(),
            stop: self.stop.clone(),
            helper: true,
            history: self.history.clone(),
            killers: [[None; 2]; MAX_PLY],
            nodes: 0,
            deadline: self.deadline,
            stopped: false,
            weights: self.weights,
            options: self.options,
        }
    }

    /// Coup gagnant immédiat du joueur au trait, s'il existe.
    fn winning_move(pos: &Position, moves: &[Move]) -> Option<Move> {
        let own = pos.bb[pos.to_move];
        let mut zones = [0u128; 41];
        let mut count = 0;
        let mut rest = own;
        while rest != 0 {
            let z = flood(rest.isolate_lowest_one(), own);
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
            s += orientations(m.shape as usize)[m.orient as usize]
                .cells
                .len() as i64;
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

    fn negamax(
        &mut self,
        pos: &Position,
        depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
    ) -> i32 {
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
        let mut tt_move = None;
        if let Some(e) = self.tt.probe(key) {
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
        let killers = self.killers[ply.min(MAX_PLY - 1)];
        let mut children: Vec<(Move, Position)> = moves
            .iter()
            .map(|&m| {
                let mut c = *pos;
                c.play(m).unwrap();
                (m, c)
            })
            .collect();
        if self.options.eval_order && depth >= 2 {
            // Le coup de la table reste en tête ; les autres, par évaluation statique.
            let skip = usize::from(tt_move.is_some());
            let p = pos.to_move;
            let w = self.weights;
            children[skip..].sort_by_cached_key(|(_, c)| {
                let s = if c.outcome.is_some() {
                    if c.winner() == Some(p) { MATE } else { 0 }
                } else {
                    let e = evaluate(c, &w);
                    if c.to_move == p { e } else { -e }
                };
                -s
            });
        }
        let alpha0 = alpha;
        let mut best = -MATE * 2;
        let mut best_move = moves[0];
        for (i, (m, child)) in children.iter().enumerate() {
            let m = *m;
            let same = child.to_move == pos.to_move;
            let search = |s: &mut Self, d: i32, a: i32, b: i32| {
                if same {
                    s.negamax(child, d, a, b, ply + 1)
                } else {
                    -s.negamax(child, d, -b, -a, ply + 1)
                }
            };
            let quiet = Some(m) != tt_move && Some(m) != killers[0] && Some(m) != killers[1];
            let reduction = if self.options.lmr && depth >= 3 && i >= 3 && quiet {
                if i >= 12 { 2 } else { 1 }
            } else {
                0
            };
            let mut score;
            if i == 0 {
                score = search(self, depth - 1, alpha, beta);
            } else {
                score = search(self, depth - 1 - reduction, alpha, alpha + 1);
                if reduction > 0 && score > alpha && !self.stopped {
                    score = search(self, depth - 1, alpha, alpha + 1);
                }
                if score > alpha && score < beta && !self.stopped {
                    score = search(self, depth - 1, alpha, beta);
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
                {
                    let h = &mut self.history[m.shape as usize][m.orient as usize][m.col as usize];
                    *h = h.saturating_add((depth * depth) as u32);
                }
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
        self.tt.store(
            key,
            Entry {
                depth: depth as i8,
                bound,
                score: best,
                best: Some(best_move),
            },
        );
        best
    }

    pub fn search(
        &mut self,
        pos: &Position,
        budget: Duration,
        max_depth: u32,
    ) -> Option<SearchResult> {
        let start = Instant::now();
        self.deadline = start + budget;
        self.stop.store(false, Relaxed);
        let mut moves = Vec::new();
        pos.legal_moves(&mut moves);
        if moves.is_empty() || pos.outcome.is_some() {
            return None;
        }
        if let Some(m) = Self::winning_move(pos, &moves) {
            return Some(SearchResult {
                best: m,
                score: MATE - 1,
                depth: 1,
                nodes: 1,
                elapsed: start.elapsed(),
            });
        }
        self.order(&mut moves, None, 0);
        let helpers = self.options.threads.max(1) - 1;
        let mut result = std::thread::scope(|scope| {
            let handles: Vec<_> = (1..=helpers)
                .map(|i| {
                    let mut h = self.spawn_helper();
                    let mut root = moves.clone();
                    let len = root.len();
                    root.rotate_left(i * 7 % len);
                    scope.spawn(move || {
                        h.iterate(pos, root, max_depth, budget, start);
                        h.nodes
                    })
                })
                .collect();
            let mut r = self.iterate(pos, moves, max_depth, budget, start);
            self.stop.store(true, Relaxed);
            for h in handles {
                r.nodes += h.join().unwrap_or(0);
            }
            r
        });
        result.elapsed = start.elapsed();
        Some(result)
    }

    fn iterate(
        &mut self,
        pos: &Position,
        mut moves: Vec<Move>,
        max_depth: u32,
        budget: Duration,
        start: Instant,
    ) -> SearchResult {
        self.stopped = false;
        self.nodes = 0;
        let done = |best, score, depth, nodes| SearchResult {
            best,
            score,
            depth,
            nodes,
            elapsed: start.elapsed(),
        };
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
                let mut score = if i == 0 {
                    run(self, alpha, beta)
                } else {
                    run(self, alpha, alpha + 1)
                };
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
                result = done(
                    m,
                    s,
                    if self.stopped { depth - 1 } else { depth },
                    self.nodes,
                );
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
            if self.options.stop_early && start.elapsed() * 3 > budget {
                break;
            }
        }
        result.nodes = self.nodes;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notation::{format_move, parse_record};

    #[test]
    fn finds_immediate_win() {
        let pos = parse_record("3I1 11 3I4 12").unwrap();
        let r = Searcher::new(16)
            .search(&pos, Duration::from_millis(500), 4)
            .unwrap();
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
        assert!(
            Searcher::winning_move(&after, &replies).is_none(),
            "{}",
            format_move(r.best)
        );
    }

    #[test]
    fn table_round_trip() {
        let t = Table::new(4);
        let e = Entry {
            depth: 7,
            bound: LOWER,
            score: -12345,
            best: Some(Move {
                shape: 6,
                orient: 7,
                col: 8,
            }),
        };
        t.store(0xDEAD_BEEF, e);
        let got = t.probe(0xDEAD_BEEF).unwrap();
        assert_eq!(
            (got.depth, got.bound, got.score, got.best),
            (7, LOWER, -12345, e.best)
        );
        assert!(t.probe(0xDEAD_BEEF ^ 1 << 40).is_none());
    }

    #[test]
    fn several_threads_return_a_legal_move() {
        let pos = parse_record("4Lr35 3Ir15").unwrap();
        let mut s = Searcher::new(16);
        s.configure("threads=3");
        let r = s.search(&pos, Duration::from_millis(300), 64).unwrap();
        let mut p = pos;
        assert!(p.play(r.best).is_ok());
    }

    #[test]
    fn always_returns_a_legal_move_from_start() {
        let pos = Position::new(0);
        let r = Searcher::new(16)
            .search(&pos, Duration::from_millis(200), 2)
            .unwrap();
        let mut p = pos;
        assert!(p.play(r.best).is_ok());
    }
}
