use rapidfuzz::fuzz;
use std::io::{BufRead, Write};

// Reads JSONL {"a":..,"b":..}; prints one JSONL line {"ratio": f64 in 0..100} per pair.
fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for line in stdin.lock().lines() {
        let v: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let a = v["a"].as_str().unwrap();
        let b = v["b"].as_str().unwrap();
        let r = fuzz::ratio(a.chars(), b.chars()) * 100.0;
        writeln!(out, "{{\"ratio\":{:?}}}", r).unwrap();
    }
}
