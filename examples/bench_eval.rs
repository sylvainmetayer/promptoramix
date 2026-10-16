use std::time::Instant;
use linkx_bot::eval::{evaluate, DEFAULT_WEIGHTS};
use linkx_bot::notation::parse_record;
fn main() {
    let pos = parse_record(&std::env::args().nth(1).unwrap_or_default()).unwrap();
    let n = 200_000;
    let t = Instant::now();
    let mut acc = 0i64;
    for _ in 0..n { acc += evaluate(std::hint::black_box(&pos), &DEFAULT_WEIGHTS) as i64; }
    println!("eval: {:.2} µs ({acc})", t.elapsed().as_secs_f64() * 1e6 / n as f64);
    let mut v = Vec::new();
    let t = Instant::now();
    for _ in 0..n { std::hint::black_box(&pos).legal_moves(&mut v); }
    println!("movegen: {:.2} µs, {} coups", t.elapsed().as_secs_f64() * 1e6 / n as f64, v.len());
    let t = Instant::now();
    for _ in 0..n/10 { for &m in &v { let mut c = pos; c.play(m).unwrap(); std::hint::black_box(c); } }
    println!("play: {:.3} µs", t.elapsed().as_secs_f64() * 1e6 / (n/10) as f64 / v.len() as f64);
}
