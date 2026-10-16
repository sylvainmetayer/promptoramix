//! Autoduel A contre B sur les ouvertures du tournoi, couleurs alternées.
//!   cargo run --release --example selfplay -- --ms 300 --threads 6 \
//!     --a "zone=30" --b "zone=60,lmr=0"
use std::sync::{Arc, Mutex};
use std::time::Duration;

use linkx_bot::game::Position;
use linkx_bot::notation::parse_record;
use linkx_bot::search::Searcher;

const OPENINGS: [&str; 17] = [
    "", "4Tr24", "4Lr38", "4Lsr15", "4Lr35", "4Lr36", "4Tr23", "4Tr21", "3Lr23", "3Lr28", "3Ir15",
    "4Tr22", "3Ir12", "3Ir17", "3Ir14", "2r15", "3Lr24",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |name: &str, d: &str| {
        args.iter()
            .position(|a| a == name)
            .map_or(d.to_string(), |i| args[i + 1].clone())
    };
    let ms: u64 = get("--ms", "300").parse().unwrap();
    let ms_b: u64 = get("--ms-b", &ms.to_string()).parse().unwrap();
    let threads: usize = get("--threads", "6").parse().unwrap();
    let rounds: usize = get("--rounds", "1").parse().unwrap();
    let (a, b) = (get("--a", ""), get("--b", ""));
    let extra: usize = get("--random", "0").parse().unwrap();
    let mut starts: Vec<Position> = (0..rounds)
        .flat_map(|_| OPENINGS.iter().map(|o| parse_record(o).unwrap()))
        .collect();
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut moves = Vec::new();
    for _ in 0..extra {
        let mut p = Position::new(0);
        for _ in 0..2 {
            p.legal_moves(&mut moves);
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            p.play(moves[(seed % moves.len() as u64) as usize]).unwrap();
        }
        starts.push(p);
    }
    let starts = Arc::new(starts);
    let jobs: Vec<(usize, usize)> = (0..starts.len()).flat_map(|g| [(g, 0), (g, 1)]).collect();
    let jobs = Arc::new(Mutex::new(jobs));
    let tally = Arc::new(Mutex::new([0u32; 3]));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let (jobs, tally, a, b) = (jobs.clone(), tally.clone(), a.clone(), b.clone());
            let starts = starts.clone();
            std::thread::spawn(move || {
                let mut sa = Searcher::new(20);
                let mut sb = Searcher::new(20);
                sa.configure(&a);
                sb.configure(&b);
                loop {
                    let Some((o, a_color)) = jobs.lock().unwrap().pop() else {
                        return;
                    };
                    let mut pos = starts[o];
                    while pos.outcome.is_none() {
                        let (s, t) = if pos.to_move == a_color {
                            (&mut sa, ms)
                        } else {
                            (&mut sb, ms_b)
                        };
                        let r = s.search(&pos, Duration::from_millis(t), 64).unwrap();
                        pos.play(r.best).unwrap();
                    }
                    let idx = match pos.winner() {
                        Some(w) if w == a_color => 0,
                        Some(_) => 1,
                        None => 2,
                    };
                    tally.lock().unwrap()[idx] += 1;
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let t = tally.lock().unwrap();
    let n = (t[0] + t[1] + t[2]) as f64;
    println!(
        "A={a:?} B={b:?} : A {} · B {} · nuls {} → A {:.1} %",
        t[0],
        t[1],
        t[2],
        100.0 * (t[0] as f64 + t[2] as f64 / 2.0) / n
    );
}
