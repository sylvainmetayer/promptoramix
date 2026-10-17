//! Rejoue une notation et cherche les bourdes d'un joueur : à chacun de ses
//! coups, une recherche plus longue donne son choix et son score ; une chute
//! de score depuis son coup précédent est marquée « ! ». Les positions
//! signalées sont candidates au livre (`examples/book_positions.txt`).
//!   cargo run --release --example blunders -- "<notation>" <blue|white> [ms] [chute]
use std::time::Duration;

use linkx_bot::game::{BLUE, WHITE};
use linkx_bot::notation::{format_move, parse_move, parse_record};
use linkx_bot::search::Searcher;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let record = args.get(1).expect("une notation");
    let player = match args.get(2).map(String::as_str) {
        Some("white") | Some("w") => WHITE,
        _ => BLUE,
    };
    let ms: u64 = args.get(3).map_or(3000, |s| s.parse().unwrap());
    let drop: i32 = args.get(4).map_or(300, |s| s.parse().unwrap());

    let tokens: Vec<&str> = record.split_whitespace().collect();
    let first = tokens.first().map(|t| t.to_ascii_lowercase());
    let skip = usize::from(matches!(
        first.as_deref(),
        Some("w" | "b" | "white" | "blue")
    ));
    let mut played = tokens[..skip].join(" ");
    let mut pos = parse_record(&played).unwrap();
    let mut s = Searcher::new(20);
    s.options.book = false;
    let mut previous: Option<i32> = None;
    for token in &tokens[skip..] {
        if *token == "--" {
            played.push_str(" --");
            continue;
        }
        let m = parse_move(token).unwrap();
        if pos.to_move == player {
            let r = s.search(&pos, Duration::from_millis(ms), 64).unwrap();
            let flag = match previous {
                Some(p) if p - r.score >= drop => " !",
                _ => "",
            };
            let other = if r.best == m {
                String::new()
            } else {
                format!(" (recherche : {})", format_move(r.best))
            };
            println!(
                "{:>7}{flag}  joué {token}{other}  prof. {}  « {} »",
                r.score,
                r.depth,
                played.trim()
            );
            previous = Some(r.score);
        }
        pos.play(m).unwrap();
        played = format!("{played} {token}").trim().to_string();
    }
    match pos.winner() {
        Some(w) => println!("issue : {} gagne", if w == BLUE { "bleu" } else { "blanc" }),
        None => println!(
            "issue : {}",
            if pos.outcome.is_some() {
                "nulle"
            } else {
                "en cours"
            }
        ),
    }
}
