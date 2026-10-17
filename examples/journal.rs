//! Positions à ajouter au livre, tirées du journal des coups du serveur
//! (`LINKX_JOURNAL` ou `docker logs`) lu sur l'entrée standard.
//!   cargo run --release --example journal -- [demi-coups max] [--all] < coups.log
//! Écrit une notation par ligne, les plus fréquentes d'abord : les positions où
//! nous avions le trait, pas plus profondes que la limite (5 par défaut, soit nos
//! trois premiers coups), et que le livre ne couvre pas encore (sauf `--all`).
//! La sortie se fusionne dans `examples/book_positions.txt` avant `make_book`.
use std::collections::HashMap;
use std::io::Read;

use linkx_bot::book::BOOK;
use linkx_bot::notation::parse_record_lenient;
use linkx_bot::search::hash;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let all = args.iter().any(|a| a == "--all");
    let max_plies: usize = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map_or(5, |a| a.parse().expect("un nombre de demi-coups"));
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();

    let mut counts: HashMap<String, usize> = HashMap::new();
    for line in input.lines() {
        let Some(record) = line.split_once('«').and_then(|(_, r)| r.split_once('»')) else {
            continue;
        };
        let record = record.0.trim();
        let plies = record
            .split_whitespace()
            .filter(|t| !matches!(*t, "w" | "b"))
            .count();
        if plies > max_plies {
            continue;
        }
        let Ok(pos) = parse_record_lenient(record) else {
            eprintln!("notation illisible : « {record} »");
            continue;
        };
        if pos.outcome.is_some() {
            continue;
        }
        let key = hash(&pos);
        if !all && BOOK.iter().any(|(k, _)| *k == key) {
            continue;
        }
        // Clé canonique : la même position écrite de plusieurs façons se cumule.
        *counts.entry(record.to_string()).or_default() += 1;
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (record, n) in &ranked {
        eprintln!("{n:>4} × « {record} »");
        println!("{record}");
    }
    eprintln!("{} position(s) à analyser", ranked.len());
}
