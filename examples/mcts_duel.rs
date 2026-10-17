//! Sparring : un joueur MCTS (UCT, simulations courtes guidées par l'évaluation)
//! contre la recherche alpha-bêta, sur les ouvertures du tournoi, couleurs
//! alternées. Un adversaire d'une autre famille que la maison : les parties
//! qu'il gagne sont imprimées en notation complète, à passer à `blunders`.
//!   cargo run --release --example mcts_duel -- --ms 300 --mcts-ms 1000 --threads 6 --rounds 1
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use linkx_bot::eval::{DEFAULT_WEIGHTS, Weights, evaluate};
use linkx_bot::game::{BLUE, Move, Position};
use linkx_bot::notation::{format_move, parse_record};
use linkx_bot::search::Searcher;

const OPENINGS: [&str; 17] = [
    "", "4Tr24", "4Lr38", "4Lsr15", "4Lr35", "4Lr36", "4Tr23", "4Tr21", "3Lr23", "3Lr28", "3Ir15",
    "4Tr22", "3Ir12", "3Ir17", "3Ir14", "2r15", "3Lr24",
];

struct Node {
    pos: Position,
    moves: Vec<Move>,
    /// Enfant par coup, créé à la première visite.
    children: Vec<Option<usize>>,
    visits: u32,
    /// Somme des résultats, en probabilité de victoire bleue.
    blue: f64,
}

struct Mcts {
    nodes: Vec<Node>,
    seed: u64,
    w: Weights,
    /// Longueur des simulations, puis l'évaluation statique tranche.
    rollout_plies: usize,
    /// Coups tirés au sort à chaque pas de simulation ; le meilleur à l'évaluation est joué.
    sample: usize,
}

/// Évaluation statique du point de vue de `player`.
fn side_score(pos: &Position, player: usize, w: &Weights) -> i32 {
    let s = evaluate(pos, w);
    if pos.to_move == player { s } else { -s }
}

/// Probabilité de victoire bleue : issue de la partie, sinon sigmoïde de l'évaluation.
fn blue_prob(pos: &Position, w: &Weights) -> f64 {
    match pos.winner() {
        Some(BLUE) => 1.0,
        Some(_) => 0.0,
        None if pos.outcome.is_some() => 0.5,
        None => 1.0 / (1.0 + (-(side_score(pos, BLUE, w) as f64) / 600.0).exp()),
    }
}

impl Mcts {
    fn rand(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn new_node(&mut self, pos: Position) -> usize {
        let mut moves = Vec::new();
        if pos.outcome.is_none() {
            pos.legal_moves(&mut moves);
        }
        let n = moves.len();
        self.nodes.push(Node {
            pos,
            moves,
            children: vec![None; n],
            visits: 0,
            blue: 0.0,
        });
        self.nodes.len() - 1
    }

    fn rollout(&mut self, start: Position) -> f64 {
        let mut pos = start;
        let mut moves = Vec::new();
        for _ in 0..self.rollout_plies {
            if pos.outcome.is_some() {
                break;
            }
            pos.legal_moves(&mut moves);
            let mover = pos.to_move;
            let mut best: Option<(i32, Move)> = None;
            for _ in 0..self.sample.min(moves.len()) {
                let m = moves[(self.rand() % moves.len() as u64) as usize];
                let mut c = pos;
                c.play(m).unwrap();
                let s = side_score(&c, mover, &self.w);
                if best.is_none_or(|(b, _)| s > b) {
                    best = Some((s, m));
                }
            }
            pos.play(best.unwrap().1).unwrap();
        }
        blue_prob(&pos, &self.w)
    }

    fn simulate(&mut self, root: usize) {
        let mut path = vec![root];
        let mut node = root;
        loop {
            if self.nodes[node].pos.outcome.is_some() {
                break;
            }
            let unexpanded: Vec<usize> = (0..self.nodes[node].moves.len())
                .filter(|&i| self.nodes[node].children[i].is_none())
                .collect();
            if !unexpanded.is_empty() {
                let i = unexpanded[(self.rand() % unexpanded.len() as u64) as usize];
                let mut c = self.nodes[node].pos;
                c.play(self.nodes[node].moves[i]).unwrap();
                let child = self.new_node(c);
                self.nodes[node].children[i] = Some(child);
                path.push(child);
                node = child;
                break;
            }
            let p = self.nodes[node].pos.to_move;
            let ln = f64::from(self.nodes[node].visits).ln();
            let mut best = (f64::MIN, node);
            for &c in self.nodes[node].children.iter().flatten() {
                let n = &self.nodes[c];
                let mean = n.blue / f64::from(n.visits);
                let mine = if p == BLUE { mean } else { 1.0 - mean };
                let u = mine + (ln / f64::from(n.visits)).sqrt();
                if u > best.0 {
                    best = (u, c);
                }
            }
            node = best.1;
            path.push(node);
        }
        let leaf = self.nodes[node].pos;
        let result = if leaf.outcome.is_some() {
            blue_prob(&leaf, &self.w)
        } else {
            self.rollout(leaf)
        };
        for &n in &path {
            self.nodes[n].visits += 1;
            self.nodes[n].blue += result;
        }
    }

    /// Coup le plus visité après `budget` de simulations.
    fn choose(&mut self, pos: &Position, budget: Duration) -> Move {
        self.nodes.clear();
        let root = self.new_node(*pos);
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            self.simulate(root);
        }
        let r = &self.nodes[root];
        let mut best = (0u32, r.moves[0]);
        for (i, c) in r.children.iter().enumerate() {
            if let Some(c) = c
                && self.nodes[*c].visits > best.0
            {
                best = (self.nodes[*c].visits, r.moves[i]);
            }
        }
        best.1
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |name: &str, d: &str| {
        args.iter()
            .position(|a| a == name)
            .map_or(d.to_string(), |i| args[i + 1].clone())
    };
    let ms: u64 = get("--ms", "300").parse().unwrap();
    let mcts_ms: u64 = get("--mcts-ms", "1000").parse().unwrap();
    let threads: usize = get("--threads", "6").parse().unwrap();
    let rounds: usize = get("--rounds", "1").parse().unwrap();
    let plies: usize = get("--plies", "12").parse().unwrap();
    let sample: usize = get("--sample", "8").parse().unwrap();
    let config = get("--config", "");

    let jobs: Vec<(usize, usize)> = (0..rounds)
        .flat_map(|_| (0..OPENINGS.len()).flat_map(|o| [(o, 0), (o, 1)]))
        .collect();
    let jobs = Arc::new(Mutex::new(jobs));
    // [victoires MCTS, victoires alpha-bêta, nuls]
    let tally = Arc::new(Mutex::new([0u32; 3]));
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            let (jobs, tally, config) = (jobs.clone(), tally.clone(), config.clone());
            std::thread::spawn(move || {
                let mut ab = Searcher::new(20);
                ab.configure(&config);
                let mut mcts = Mcts {
                    nodes: Vec::new(),
                    seed: 0x9E37_79B9_7F4A_7C15 ^ ((t as u64 + 1) * 0x2545_F491_4F6C_DD1D),
                    w: DEFAULT_WEIGHTS,
                    rollout_plies: plies,
                    sample,
                };
                loop {
                    let Some((o, mcts_color)) = jobs.lock().unwrap().pop() else {
                        return;
                    };
                    let mut pos = parse_record(OPENINGS[o]).unwrap();
                    let mut record = OPENINGS[o].to_string();
                    while pos.outcome.is_none() {
                        let mover = pos.to_move;
                        let m = if mover == mcts_color {
                            mcts.choose(&pos, Duration::from_millis(mcts_ms))
                        } else {
                            ab.search(&pos, Duration::from_millis(ms), 64).unwrap().best
                        };
                        pos.play(m).unwrap();
                        record = format!("{record} {}", format_move(m)).trim().to_string();
                        if pos.outcome.is_none() && pos.to_move == mover {
                            record.push_str(" --");
                        }
                    }
                    let idx = match pos.winner() {
                        Some(w) if w == mcts_color => 0,
                        Some(_) => 1,
                        None => 2,
                    };
                    tally.lock().unwrap()[idx] += 1;
                    if idx == 0 {
                        let color = if mcts_color == BLUE { "blue" } else { "white" };
                        println!("MCTS gagne en {color} : {record}");
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let t = tally.lock().unwrap();
    println!(
        "MCTS {mcts_ms} ms (simulations de {plies} demi-coups, {sample} coups tirés) contre alpha-bêta {ms} ms : \
         MCTS {} · alpha-bêta {} · nuls {}",
        t[0], t[1], t[2]
    );
}
