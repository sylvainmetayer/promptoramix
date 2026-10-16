//! Recherche à profondeur fixe sur quelques positions : nœuds et temps.
//!   cargo run --release --example bench_search [profondeur]
use std::time::{Duration, Instant};

use linkx_bot::notation::parse_record;
use linkx_bot::search::Searcher;

const POSITIONS: [&str; 5] = [
    "",
    "4Tr24",
    "4Lr35 3Ir15 3Ir15 4Lr38",
    "4Lsr11 4Lsr17 4Lsr37 4Lr35 3Ir12 2r12",
    "4Tr21 4Tr25 3Ir12 2r16 3Ir12 12 2r16 4Sr17",
];

fn main() {
    let depth: u32 = std::env::args().nth(1).map_or(6, |d| d.parse().unwrap());
    let (mut nodes, mut total) = (0u64, Duration::ZERO);
    for record in POSITIONS {
        let pos = parse_record(record).unwrap();
        let mut s = Searcher::new(20);
        s.options.book = false;
        s.configure(&std::env::var("LINKX_CONFIG").unwrap_or_default());
        let t = Instant::now();
        let r = s.search(&pos, Duration::from_secs(600), depth).unwrap();
        let e = t.elapsed();
        nodes += r.nodes;
        total += e;
        println!(
            "{record:<45} prof {} score {:>7} {:>8} nœuds {:>6} ms",
            r.depth,
            r.score,
            r.nodes,
            e.as_millis()
        );
    }
    println!(
        "TOTAL {nodes} nœuds en {} ms : {:.0} nœuds/s",
        total.as_millis(),
        nodes as f64 / total.as_secs_f64()
    );
}
