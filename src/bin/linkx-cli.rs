//! `linkx-cli "<notation>" [--ms N] [--depth N]` : écrit le coup choisi sur la sortie.

use std::time::Duration;

use linkx_bot::notation::{format_move, parse_record};
use linkx_bot::search::Searcher;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str, default: u64| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map_or(default, |v| v.parse().expect("nombre attendu"))
    };
    let record = args.first().filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let pos = match parse_record(&record) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("notation refusée au coup {} ({}) : {}", e.index, e.token, e.reason.code());
            std::process::exit(2);
        }
    };
    let mut searcher = Searcher::new(flag("--tt", 22) as u32);
    let budget = Duration::from_millis(flag("--ms", 4500));
    let Some(r) = searcher.search(&pos, budget, flag("--depth", 64) as u32) else {
        eprintln!("aucun coup à jouer");
        std::process::exit(3);
    };
    eprintln!("depth={} score={} nodes={} ms={}", r.depth, r.score, r.nodes, r.elapsed.as_millis());
    println!("{}", format_move(r.best));
}
