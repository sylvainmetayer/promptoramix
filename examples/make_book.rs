//! Génère `src/book.rs` : coup de recherche longue pour chaque position de
//! départ du tournoi (plateau vide pour chaque couleur, ouvertures imposées).
//!   cargo run --release --example make_book -- 240 8 examples/book_positions.txt
//! Les positions sont celles des ouvertures disputées contre la maison (nos deux
//! premiers coups), relevées dans les journaux de duel.
use std::time::Duration;

use linkx_bot::notation::{format_move, parse_record};
use linkx_bot::search::Searcher;

const STARTS: [&str; 18] = [
    "", "w", "4Tr24", "4Lr38", "4Lsr15", "4Lr35", "4Lr36", "4Tr23", "4Tr21", "3Lr23", "3Lr28",
    "3Ir15", "4Tr22", "3Ir12", "3Ir17", "3Ir14", "2r15", "3Lr24",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args.get(1).map_or(60, |s| s.parse().unwrap());
    let workers: usize = args.get(2).map_or(1, |s| s.parse().unwrap());
    // Positions à analyser : un fichier (une notation par ligne) ou les départs du tournoi.
    let records: Vec<String> = match args.get(3) {
        Some(path) => std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect(),
        None => STARTS.iter().map(|s| s.to_string()).collect(),
    };
    // Un fil par position : la recherche parallèle ne gagne pas de profondeur.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let lines = std::sync::Mutex::new(vec![String::new(); records.len()]);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(record) = records.get(i) else { return };
                    let pos = parse_record(record).unwrap();
                    let mut s = Searcher::new(22);
                    s.options.book = false;
                    let r = s.search(&pos, Duration::from_secs(secs), 64).unwrap();
                    let mv = format_move(r.best);
                    eprintln!("{record:<24} -> {mv:<7} prof {} score {}", r.depth, r.score);
                    lines.lock().unwrap()[i] = format!(
                        "    (0x{:016x}, \"{mv}\"), // « {record} », prof. {}\n",
                        pos.key, r.depth
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
    for line in lines.into_inner().unwrap() {
        out.push_str(&line);
    }
    out.push_str("];\n");
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/book.rs"), out).unwrap();
}
