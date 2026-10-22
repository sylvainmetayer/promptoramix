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
//! Une position où le livre joue déjà un autre coup (correction d'une vague
//! précédente, l'export gardant les anciennes parties) n'est pas répétée, ni
//! une position que le livre ne laisse plus atteindre.
//!
//! Sur la sortie d'erreur : le bilan, et toute partie gagnée dont un coup
//! diffère de ce que le livre jouerait aujourd'hui (un livre qui toucherait une
//! victoire mérite d'être regardé avant la vague suivante).
use std::collections::{BTreeMap, BTreeSet};

use linkx_bot::notation::{format_move, parse_move, parse_record, tokens};
use linkx_bot::search::book_lookup;
use serde_json::Value;

#[derive(Default)]
struct Node {
    won: usize,
    lost: usize,
    drawn: usize,
    played: BTreeSet<String>,
    parent: Option<String>,
    /// Coup que le livre joue aujourd'hui dans cette position.
    book: Option<String>,
    /// Parties qui atteignent encore ce nœud : aucun coup du livre plus tôt
    /// dans la partie ne s'écarte de ce qui a été joué.
    reachable: usize,
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
        // Mêmes séparateurs que `parse_record`. Les passes se déduisent de la
        // position : on les retire, le préfixe reste lisible en lecture stricte.
        let words: Vec<&str> = tokens(record).filter(|t| *t != "--").collect();
        // Indication de premier joueur (`w 4Tr21…`) : gardée dans le préfixe,
        // ce n'est pas un coup.
        let lead = usize::from(words.first().is_some_and(|t| {
            matches!(
                t.to_ascii_lowercase().as_str(),
                "b" | "blue" | "w" | "white"
            )
        }));
        let Ok(mut pos) = parse_record(&words[..lead].join(" ")) else {
            continue;
        };
        let mut parent: Option<String> = None;
        // Le livre s'écarte-t-il de cette partie avant le coup courant ?
        let mut diverged = false;
        for i in lead..words.len() {
            if pos.outcome.is_some() {
                break;
            }
            let Ok(m) = parse_move(words[i]) else {
                eprintln!("notation illisible : {color} « {record} » ({})", words[i]);
                break;
            };
            if pos.to_move == ours {
                let prefix = words[..i].join(" ");
                let played = format_move(m);
                let key = format!("{color}|{prefix}");
                let node = nodes.entry(key.clone()).or_insert_with(|| Node {
                    book: book_lookup(&pos).map(format_move),
                    ..Node::default()
                });
                match issue {
                    "gagne" => node.won += 1,
                    "perdu" => node.lost += 1,
                    _ => node.drawn += 1,
                }
                node.played.insert(played.clone());
                if !diverged {
                    node.reachable += 1;
                }
                if node.parent.is_none() {
                    node.parent = parent.clone();
                }
                // Un coup de livre qui diffère d'un coup gagnant change la partie.
                if issue == "gagne"
                    && let Some(book) = &node.book
                    && *book != played
                {
                    eprintln!(
                        "victoire touchée par le livre : {color} « {prefix} » joué {played}, livre {book}"
                    );
                }
                if node.book.as_ref().is_some_and(|b| *b != played) {
                    diverged = true;
                }
                parent = Some(key);
            }
            if pos.play(m).is_err() {
                eprintln!("coup illégal : {color} « {record} » ({})", words[i]);
                break;
            }
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
        // Le livre quitte ces parties plus tôt : la position ne se présentera
        // plus contre les mêmes adversaires.
        if node.reachable == 0 {
            eprintln!("inaccessible : {color} « {prefix} », le livre en sort plus tôt");
            continue;
        }
        // Le livre y joue déjà un autre coup (vague précédente) : la partie
        // perdue ne se rejouera pas, et sa ligne figure déjà au fichier.
        if let Some(book) = node.book.as_ref().filter(|b| !node.played.contains(*b)) {
            eprintln!("déjà corrigée : {color} « {prefix} », livre {book}");
            continue;
        }
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
