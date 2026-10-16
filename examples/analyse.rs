//! Rejoue une notation et affiche, coup par coup, distances, zones et plateau.
use linkx_bot::eval::{DEFAULT_WEIGHTS, distances, evaluate, remaining_cells};
use linkx_bot::game::largest_zone;
use linkx_bot::notation::{parse_move, parse_record};

fn main() {
    let record = std::env::args().nth(1).unwrap();
    let mut pos = parse_record("").unwrap();
    let w = DEFAULT_WEIGHTS;
    for (i, tok) in record.split_whitespace().enumerate() {
        if tok == "--" {
            continue;
        }
        let mover = if pos.to_move == 0 { "B" } else { "W" };
        pos.play(parse_move(tok).unwrap()).unwrap();
        let d: Vec<_> = (0..2).map(|p| distances(&pos, p, &w)).collect();
        println!(
            "{:>2} {mover} {tok:<7} distB={:?} distW={:?} zones={}/{} reste={}/{} eval(trait)={}",
            i + 1,
            d[0],
            d[1],
            largest_zone(pos.bb[0]),
            largest_zone(pos.bb[1]),
            remaining_cells(&pos, 0),
            remaining_cells(&pos, 1),
            evaluate(&pos, &w)
        );
    }
    print!("{}", pos.render());
    println!("{:?}", pos.outcome);
}
