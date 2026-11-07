use std::io::Write;
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

// Usage: scan-spike DIR [DIR...]; JSONL to stdout, summary JSON to stderr. Read-only.
fn walk(dir: &Path, files: &mut Vec<std::path::PathBuf>, errs: &mut u64) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => {
            *errs += 1;
            return;
        }
    };
    for e in rd.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, files, errs),
            Ok(t) if t.is_file() => {
                if p.extension().map_or(false, |x| x.eq_ignore_ascii_case("mp4")) {
                    files.push(p)
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let start = Instant::now();
    let mut files = Vec::new();
    let mut walk_errs = 0u64;
    for d in std::env::args().skip(1) {
        walk(Path::new(&d), &mut files, &mut walk_errs);
    }
    files.sort();
    let out = std::io::stdout();
    let mut out = std::io::BufWriter::new(out.lock());
    let (mut tagged, mut untagged, mut errors) = (0u64, 0u64, 0u64);
    for p in &files {
        let md = std::fs::metadata(p).ok();
        let size = md.as_ref().map(|m| m.len());
        let mtime = md.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());
        let (mut pid, mut sid, mut tag_error) = (None, None, None);
        match mp4ameta::Tag::read_from_path(p) {
            Ok(tag) => match tag.comment() {
                Some(c) => {
                    let parts: Vec<&str> = c.split(':').collect();
                    if parts.len() >= 3 && parts[0] == "ruvinfo" {
                        pid = Some(parts[1].to_string());
                        sid = Some(parts[2].to_string());
                        tagged += 1;
                    } else {
                        untagged += 1;
                    }
                }
                None => untagged += 1,
            },
            Err(e) => {
                tag_error = Some(e.to_string());
                errors += 1;
            }
        }
        let line = serde_json::json!({"path": p.to_string_lossy(), "size": size, "mtime": mtime, "pid": pid, "sid": sid, "tag_error": tag_error});
        writeln!(out, "{}", line).unwrap();
    }
    out.flush().unwrap();
    eprintln!(
        "{}",
        serde_json::json!({"files": files.len(), "tagged": tagged, "untagged": untagged, "errors": errors, "walk_errors": walk_errs, "elapsed_ms": start.elapsed().as_millis()})
    );
}
