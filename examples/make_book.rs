//! Génère `src/book.rs` : coup de recherche longue pour chaque position de
//! départ du tournoi (plateau vide pour chaque couleur, ouvertures imposées).
//!   cargo run --release --example make_book -- <secondes par position> [fils]
use std::fmt::Write;
use std::time::Duration;

use linkx_bot::notation::{format_move, parse_record};
use linkx_bot::search::{Searcher, hash};

const STARTS: [&str; 18] = [
    "", "w", "4Tr24", "4Lr38", "4Lsr15", "4Lr35", "4Lr36", "4Tr23", "4Tr21", "3Lr23", "3Lr28",
    "3Ir15", "4Tr22", "3Ir12", "3Ir17", "3Ir14", "2r15", "3Lr24",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args.get(1).map_or(60, |s| s.parse().unwrap());
    let threads: usize = args.get(2).map_or(1, |s| s.parse().unwrap());
    let mut out = String::from(
        "//! Généré par `cargo run --release --example make_book`, ne pas éditer.\n\
         //! Empreinte Zobrist de la position -> coup, en notation canonique.\n\n\
         pub const BOOK: &[(u64, &str)] = &[\n",
    );
    for record in STARTS {
        let pos = parse_record(record).unwrap();
        let mut s = Searcher::new(22);
        s.options.threads = threads;
        s.options.book = false;
        let r = s.search(&pos, Duration::from_secs(secs), 64).unwrap();
        let mv = format_move(r.best);
        eprintln!("{record:<6} -> {mv:<7} prof {} score {}", r.depth, r.score);
        writeln!(
            out,
            "    (0x{:016x}, \"{mv}\"), // « {record} », prof. {}",
            hash(&pos),
            r.depth
        )
        .unwrap();
    }
    out.push_str("];\n");
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/src/book.rs"), out).unwrap();
}
