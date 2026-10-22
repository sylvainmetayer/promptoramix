//! Génère `src/book.rs` : coup de recherche longue pour chaque position de
//! départ du tournoi (plateau vide pour chaque couleur, ouvertures imposées).
//!   cargo run --release --example make_book -- 240 8 examples/book_positions.txt [--keep]
//! Les positions sont celles des ouvertures disputées contre la maison (nos deux
//! premiers coups), relevées dans les journaux de duel, puis celles des parties
//! perdues en tournoi.
//!
//! Une ligne du fichier est une notation, éventuellement suivie de `! <coups>` :
//! ces coups sont écartés de la recherche. C'est ainsi qu'on remplace un coup
//! qui a perdu contre un adversaire déterministe, qui rejouerait la même partie.
//!
//! `--keep` reprend telles quelles les entrées de `src/book.rs` dont la ligne
//! n'a pas changé, et ne cherche que les nouvelles.
use std::collections::HashMap;
use std::time::Duration;

use linkx_bot::notation::{format_move, parse_move, parse_record};
use linkx_bot::search::Searcher;

const STARTS: [&str; 18] = [
    "", "w", "4Tr24", "4Lr38", "4Lsr15", "4Lr35", "4Lr36", "4Tr23", "4Tr21", "3Lr23", "3Lr28",
    "3Ir15", "4Tr22", "3Ir12", "3Ir17", "3Ir14", "2r15", "3Lr24",
];

const BOOK_RS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/book.rs");

/// Clé d'une entrée : la notation et les coups écartés, tels qu'écrits dans le
/// commentaire de `book.rs` (`« notation »` puis `sauf …`).
fn entry_label(record: &str, excluded: &[String]) -> String {
    if excluded.is_empty() {
        format!("« {record} »")
    } else {
        format!("« {record} » sauf {}", excluded.join(" "))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let keep = args.iter().any(|a| a == "--keep");
    let args: Vec<&String> = args.iter().filter(|a| *a != "--keep").collect();
    let secs: u64 = args.get(1).map_or(60, |s| s.parse().unwrap());
    let workers: usize = args.get(2).map_or(1, |s| s.parse().unwrap());
    // Positions à analyser : un fichier (une notation par ligne) ou les départs du tournoi.
    let lines: Vec<String> = match args.get(3) {
        Some(path) => std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect(),
        None => STARTS.iter().map(|s| s.to_string()).collect(),
    };
    let entries: Vec<(String, Vec<String>)> = lines
        .iter()
        .map(|line| match line.split_once('!') {
            Some((record, moves)) => (
                record.trim().to_string(),
                moves.split_whitespace().map(str::to_string).collect(),
            ),
            None => (line.trim().to_string(), Vec::new()),
        })
        .collect();
    // Deux lignes pour une même position : le livre ne lirait que la première,
    // la correction `!` d'une partie perdue serait masquée sans un mot.
    let mut seen = std::collections::HashSet::new();
    for (record, _) in &entries {
        assert!(
            seen.insert(record.as_str()),
            "« {record} » figure deux fois : remplacer l'ancienne ligne"
        );
    }
    // Entrées déjà calculées, par étiquette, pour `--keep`.
    // Commentaire d'une entrée : `« notation » [sauf …][, score S], prof. N`.
    let existing: HashMap<String, String> = if keep {
        std::fs::read_to_string(BOOK_RS)
            .unwrap()
            .lines()
            .filter_map(|l| {
                let comment = l.split_once("// ")?.1;
                let label = comment.rsplit_once(", prof.")?.0;
                let label = label.rsplit_once(", score").map_or(label, |(l, _)| l);
                Some((label.to_string(), format!("{l}\n")))
            })
            .collect()
    } else {
        HashMap::new()
    };
    let mut out_lines = vec![String::new(); entries.len()];
    let mut todo = Vec::new();
    for (i, (record, excluded)) in entries.iter().enumerate() {
        match existing.get(&entry_label(record, excluded)) {
            Some(line) => out_lines[i] = line.clone(),
            None => todo.push(i),
        }
    }
    eprintln!(
        "{} entrée(s) reprise(s), {} à chercher",
        entries.len() - todo.len(),
        todo.len()
    );
    // Un fil par position : la recherche parallèle ne gagne pas de profondeur.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out_lines = std::sync::Mutex::new(out_lines);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&i) = todo.get(k) else { return };
                    let (record, excluded) = &entries[i];
                    let pos = parse_record(record).unwrap();
                    let mut s = Searcher::new(22);
                    s.options.book = false;
                    s.excluded = excluded
                        .iter()
                        .map(|t| parse_move(t).expect("coup écarté illisible"))
                        .collect();
                    let r = s.search(&pos, Duration::from_secs(secs), 64).unwrap();
                    let mv = format_move(r.best);
                    let label = entry_label(record, excluded);
                    eprintln!("{label:<40} -> {mv:<7} prof {} score {}", r.depth, r.score);
                    out_lines.lock().unwrap()[i] = format!(
                        "    (0x{:016x}, \"{mv}\"), // {label}, score {}, prof. {}\n",
                        pos.key, r.score, r.depth
                    );
                }
            });
        }
    });
    let mut out = String::from(
        "//! Généré par `cargo run --release --example make_book`, ne pas éditer.\n\
         //! Empreinte Zobrist de la position -> coup, en notation canonique.\n\n\
         pub const BOOK: &[(u64, &str)] = &[\n",
    );
    for line in out_lines.into_inner().unwrap() {
        out.push_str(&line);
    }
    out.push_str("];\n");
    std::fs::write(BOOK_RS, out).unwrap();
}
