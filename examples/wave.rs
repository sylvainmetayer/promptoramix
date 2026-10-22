//! Bilan d'une vague du tournoi, depuis l'export JSON de la plateforme
//! (« Mes parties », format `linkx-parties`).
//!   cargo run --release --example wave -- parties.json
//!
//! Les adversaires sont pour la plupart déterministes, comme nous : une partie
//! perdue se rejouera à l'identique tant que personne ne change de coup. On
//! range nos positions en arbre et on relève, pour chaque ligne perdue, le
//! premier de nos coups par lequel ne passe aucune partie gagnée : y changer de
//! coup ne touche à aucune victoire. Ces positions sortent sur la sortie
//! standard au format de `examples/book_positions.txt` (`notation ! coups`, les
//! coups joués étant écartés), à analyser ensuite par `make_book --keep`.
//!
//! Sur la sortie d'erreur : le bilan, et toute partie gagnée dont un coup
//! diffère de ce que le livre jouerait aujourd'hui (un livre qui toucherait une
//! victoire mérite d'être regardé avant la vague suivante).
use std::collections::{BTreeMap, BTreeSet};

use linkx_bot::notation::{format_move, parse_move, parse_record};
use linkx_bot::search::book_lookup;
use serde_json::Value;

#[derive(Default)]
struct Node {
    won: usize,
    lost: usize,
    drawn: usize,
    played: BTreeSet<String>,
    parent: Option<String>,
}

fn main() {
    let path = std::env::args().nth(1).expect("chemin de l'export JSON");
    let export: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let games = export["parties"].as_array().expect("champ « parties »");

    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mut tally = BTreeMap::<(String, String), usize>::new();
    for game in games {
        let color = game["couleur"].as_str().unwrap_or_default();
        let issue = game["issue"].as_str().unwrap_or_default();
        let record = game["notation"].as_str().unwrap_or_default();
        *tally
            .entry((color.to_string(), issue.to_string()))
            .or_default() += 1;
        let ours = if color == "white" { 1 } else { 0 };
        let tokens: Vec<&str> = record.split_whitespace().collect();
        let mut parent: Option<String> = None;
        for i in 0..tokens.len() {
            if tokens[i] == "--" {
                continue;
            }
            let prefix = tokens[..i].join(" ");
            let Ok(pos) = parse_record(&prefix) else {
                break;
            };
            if pos.to_move != ours || pos.outcome.is_some() {
                continue;
            }
            let played = parse_move(tokens[i]).map_or(tokens[i].to_string(), format_move);
            let key = format!("{color}|{prefix}");
            let node = nodes.entry(key.clone()).or_default();
            match issue {
                "gagne" => node.won += 1,
                "perdu" => node.lost += 1,
                _ => node.drawn += 1,
            }
            node.played.insert(played.clone());
            if node.parent.is_none() {
                node.parent = parent.clone();
            }
            // Un coup de livre qui diffère d'un coup gagnant change la partie.
            if issue == "gagne"
                && let Some(book) = book_lookup(&pos)
                && format_move(book) != played
            {
                eprintln!(
                    "victoire touchée par le livre : {color} « {prefix} » joué {played}, livre {}",
                    format_move(book)
                );
            }
            parent = Some(key);
        }
    }

    for ((color, issue), n) in &tally {
        eprintln!("{color:<6} {issue:<6} {n}");
    }
    let mut repairs = 0;
    for (key, node) in &nodes {
        if node.won > 0 {
            continue;
        }
        let parent_clean = node
            .parent
            .as_ref()
            .and_then(|p| nodes.get(p))
            .is_some_and(|p| p.won == 0);
        if parent_clean {
            continue;
        }
        let (color, prefix) = key.split_once('|').unwrap();
        let played: Vec<&str> = node.played.iter().map(String::as_str).collect();
        eprintln!(
            "à corriger : {color} « {prefix} » joué {} ({} perdue(s), {} nulle(s))",
            played.join(", "),
            node.lost,
            node.drawn
        );
        println!("{prefix} ! {}", played.join(" "));
        repairs += 1;
    }
    eprintln!("{repairs} position(s) à corriger");
}
