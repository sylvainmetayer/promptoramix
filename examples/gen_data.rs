//! Parties d'autoduel pour l'ajustement de l'évaluation (méthode Texel).
//! Une ligne par position : notation depuis le début, résultat du point de vue
//! de bleu (1, 0,5 ou 0).
//!   cargo run --release --example gen_data -- <parties> <ms par coup> <fils> > data.tsv
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use linkx_bot::game::Position;
use linkx_bot::notation::format_move;
use linkx_bot::search::Searcher;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let games: usize = args.get(1).map_or(2000, |s| s.parse().unwrap());
    let ms: u64 = args.get(2).map_or(150, |s| s.parse().unwrap());
    let threads: usize = args.get(3).map_or(6, |s| s.parse().unwrap());
    let next = Arc::new(AtomicUsize::new(0));
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let (next, out) = (next.clone(), out.clone());
            std::thread::spawn(move || {
                let mut moves = Vec::new();
                loop {
                    let g = next.fetch_add(1, Ordering::Relaxed);
                    if g >= games {
                        return;
                    }
                    let mut seed =
                        0x9E37_79B9_7F4A_7C15u64 ^ (g as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
                    let mut rand = || {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        seed
                    };
                    let mut pos = Position::new(g % 2);
                    let mut record = if g % 2 == 1 {
                        vec!["w".to_string()]
                    } else {
                        vec![]
                    };
                    let mut positions = Vec::new();
                    // Deux à quatre coups au hasard, puis la recherche.
                    let random_plies = 2 + (rand() % 3) as usize;
                    let mut ply = 0;
                    let mut searcher = Searcher::new(18);
                    while pos.outcome.is_none() {
                        let m = if ply < random_plies {
                            pos.legal_moves(&mut moves);
                            moves[(rand() % moves.len() as u64) as usize]
                        } else {
                            searcher
                                .search(&pos, Duration::from_millis(ms), 64)
                                .unwrap()
                                .best
                        };
                        let before = pos.to_move;
                        pos.play(m).unwrap();
                        record.push(format_move(m));
                        if pos.outcome.is_none() && pos.to_move == before {
                            record.push("--".into());
                        }
                        ply += 1;
                        if ply > random_plies && pos.outcome.is_none() {
                            positions.push(record.join(" "));
                        }
                    }
                    let result = match pos.winner() {
                        Some(0) => "1",
                        Some(_) => "0",
                        None => "0.5",
                    };
                    let mut o = out.lock().unwrap();
                    for p in positions {
                        writeln!(o, "{p}\t{result}").unwrap();
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}
