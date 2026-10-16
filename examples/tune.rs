//! Ajustement Texel des poids d'évaluation sur des positions d'autoduel
//! (`gen_data`) : minimise l'écart quadratique entre sigmoïde(K · éval) et le
//! résultat final, par descente de coordonnées.
//!   cargo run --release --example tune -- data.tsv
use linkx_bot::eval::{DEFAULT_WEIGHTS, RING, Weights, evaluate, max_step};
use linkx_bot::game::Position;
use linkx_bot::notation::parse_record;

fn loss(data: &[(Position, f64)], w: &Weights, k: f64) -> f64 {
    let total: f64 = data
        .iter()
        .map(|(pos, r)| {
            let e = evaluate(pos, w) as f64;
            let blue = if pos.to_move == 0 { e } else { -e };
            let p = 1.0 / (1.0 + (-k * blue).exp());
            (r - p) * (r - p)
        })
        .sum();
    total / data.len() as f64
}

/// Constante K de la sigmoïde qui minimise la perte de ces poids.
fn fit_k(data: &[(Position, f64)], w: &Weights) -> (f64, f64) {
    (1..80)
        .map(|i| 0.00005 * i as f64)
        .map(|c| (c, loss(data, w, c)))
        .fold((0.0, f64::MAX), |a, b| if b.1 < a.1 { b } else { a })
}

type Field = (&'static str, fn(&mut Weights) -> &mut i32, i32, i32);

fn main() {
    let path = std::env::args().nth(1).expect("fichier de positions");
    let text = std::fs::read_to_string(path).unwrap();
    let data: Vec<(Position, f64)> = text
        .lines()
        .filter_map(|l| {
            let (rec, r) = l.split_once('\t')?;
            Some((parse_record(rec).ok()?, r.parse().ok()?))
        })
        .collect();
    eprintln!("{} positions", data.len());

    let mut w = DEFAULT_WEIGHTS;
    // Contrôle seul : perte des poids par défaut et d'un réglage donné.
    if let Some(spec) = std::env::args().nth(2) {
        let mut s = linkx_bot::search::Searcher::new(4);
        s.configure(&spec);
        for (name, weights) in [("défaut", DEFAULT_WEIGHTS), ("réglage", s.weights)] {
            let best = fit_k(&data, &weights);
            println!("{name:<8} K={:.5} perte {:.6}", best.0, best.1);
        }
        return;
    }
    let (k, initial) = fit_k(&data, &w);
    eprintln!("K = {k:.5}, perte initiale {initial:.6}");

    let fields: [Field; 9] = [
        ("empty", |w| &mut w.empty, 1, 12),
        ("gap", |w| &mut w.gap, 0, 7),
        ("distance", |w| &mut w.distance, 5, 200),
        ("second", |w| &mut w.second_axis, 0, 40),
        ("zone", |w| &mut w.zone, 0, 300),
        ("tempo", |w| &mut w.tempo, -500, 1500),
        ("nok", |w| &mut w.narrow_ok, 0, 30),
        ("nnone", |w| &mut w.narrow_none, 0, 40),
        ("nkeep", |w| &mut w.narrow_keep, 0, 300),
    ];
    let valid = |w: &Weights| (max_step(w) as usize) < RING;
    let mut current = initial;
    for round in 0..6 {
        let mut improved = false;
        for (name, field, lo, hi) in fields {
            let span = (hi - lo).max(1);
            for step in [span / 8, span / 32, 1].into_iter().filter(|s| *s > 0) {
                loop {
                    let mut moved = false;
                    for dir in [step, -step] {
                        let mut t = w;
                        let v = field(&mut t);
                        let old = *v;
                        *v = (old + dir).clamp(lo, hi);
                        // À une borne, l'essai redonnerait les mêmes poids.
                        if *v == old || !valid(&t) {
                            continue;
                        }
                        let l = loss(&data, &t, k);
                        if l < current - 1e-7 {
                            w = t;
                            current = l;
                            moved = true;
                            improved = true;
                            break;
                        }
                    }
                    if !moved {
                        break;
                    }
                }
            }
            eprintln!(
                "tour {round} {name:<8} = {:>5}  perte {current:.6}",
                *field(&mut w.clone())
            );
        }
        for (name, flag) in [("cliff", 0), ("zphase", 1), ("vstep", 2), ("wells3", 3)] {
            let mut t = w;
            let b = match flag {
                0 => &mut t.cliff,
                1 => &mut t.zone_phase,
                2 => &mut t.vstep,
                _ => &mut t.wells3,
            };
            *b = !*b;
            let l = loss(&data, &t, k);
            if l < current - 1e-7 {
                w = t;
                current = l;
                improved = true;
            }
            eprintln!("tour {round} {name:<8} essai {l:.6}");
        }
        if !improved {
            break;
        }
    }
    println!(
        "empty={},gap={},distance={},second={},zone={},tempo={},nok={},nnone={},nkeep={},cliff={},zphase={},vstep={},wells3={}",
        w.empty,
        w.gap,
        w.distance,
        w.second_axis,
        w.zone,
        w.tempo,
        w.narrow_ok,
        w.narrow_none,
        w.narrow_keep,
        w.cliff as i32,
        w.zone_phase as i32,
        w.vstep as i32,
        w.wells3 as i32
    );
    eprintln!("perte finale {current:.6}");
}
