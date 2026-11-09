//! `fuzzywuzzy.fuzz.ratio` and `partial_ratio` as ruvsarpur runs them:
//! fuzzywuzzy 0.18 over python-Levenshtein 0.27, which is RapidFuzz.
//!
//! `ratio` is RapidFuzz's normalised Indel similarity. `partial_ratio`
//! scores the windows of the longer string that line up with the matching
//! blocks of a Levenshtein alignment, so the alignment has to be RapidFuzz's
//! own (`Levenshtein.editops`: common affix removed, Hyyrö's bit vectors,
//! the same backtrace). The vertical deltas Hyyrö keeps as bits are read
//! here from the plain DP matrix, which holds the same numbers.
//!
//! Strings are compared by Unicode scalar, like Python's `str`.

/// `rapidfuzz.distance.Indel.normalized_similarity`.
fn indel_ratio(a: &[char], b: &[char]) -> f64 {
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    let distance = total - 2 * lcs(a, b);
    1.0 - distance as f64 / total as f64
}

fn lcs(a: &[char], b: &[char]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for ca in a {
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = if ca == cb {
                prev[j] + 1
            } else {
                cur[j].max(prev[j + 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// `utils.intr`: `int(round(n))`, rounding halves to even like Python.
fn intr(n: f64) -> i64 {
    n.round_ties_even() as i64
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tag {
    Replace,
    Insert,
    Delete,
}

#[derive(Debug, Clone, Copy)]
struct Editop {
    tag: Tag,
    src: usize,
    dest: usize,
}

/// `rapidfuzz.distance.Levenshtein.editops` (the reference implementation
/// in `Levenshtein_py.py`, which the C++ one matches).
fn editops(s1: &[char], s2: &[char]) -> Vec<Editop> {
    let prefix = s1.iter().zip(s2).take_while(|(a, b)| a == b).count();
    let (r1, r2) = (&s1[prefix..], &s2[prefix..]);
    let suffix = r1
        .iter()
        .rev()
        .zip(r2.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let a = &r1[..r1.len() - suffix];
    let b = &r2[..r2.len() - suffix];
    let (n, m) = (a.len(), b.len());

    // d[i][j]: distance between a[..i] and b[..j].
    let w = m + 1;
    let mut d = vec![0usize; (n + 1) * w];
    for i in 0..=n {
        d[i * w] = i;
    }
    for (j, cell) in d.iter_mut().enumerate().take(m + 1) {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i * w + j] = (d[(i - 1) * w + j] + 1)
                .min(d[i * w + j - 1] + 1)
                .min(d[(i - 1) * w + j - 1] + cost);
        }
    }
    let mut dist = d[n * w + m];
    if a.is_empty() {
        dist = m;
    }
    if dist == 0 {
        return Vec::new();
    }
    // Hyyrö's VP/VN bit for pattern position `col` after text position `row`.
    let delta =
        |col: usize, row: usize| d[col * w + row] as isize - d[(col - 1) * w + row] as isize;
    let vp = |row: usize, col: usize| delta(col, row) == 1;
    let vn = |row: usize, col: usize| delta(col, row) == -1;

    let mut ops = vec![
        Editop {
            tag: Tag::Replace,
            src: 0,
            dest: 0
        };
        dist
    ];
    let (mut col, mut row) = (n, m);
    while row != 0 && col != 0 {
        if vp(row, col) {
            dist -= 1;
            col -= 1;
            ops[dist] = Editop {
                tag: Tag::Delete,
                src: col + prefix,
                dest: row + prefix,
            };
        } else {
            row -= 1;
            if row != 0 && vn(row, col) {
                dist -= 1;
                ops[dist] = Editop {
                    tag: Tag::Insert,
                    src: col + prefix,
                    dest: row + prefix,
                };
            } else {
                col -= 1;
                if a[col] != b[row] {
                    dist -= 1;
                    ops[dist] = Editop {
                        tag: Tag::Replace,
                        src: col + prefix,
                        dest: row + prefix,
                    };
                }
            }
        }
    }
    while col != 0 {
        dist -= 1;
        col -= 1;
        ops[dist] = Editop {
            tag: Tag::Delete,
            src: col + prefix,
            dest: row + prefix,
        };
    }
    while row != 0 {
        dist -= 1;
        row -= 1;
        ops[dist] = Editop {
            tag: Tag::Insert,
            src: col + prefix,
            dest: row + prefix,
        };
    }
    ops
}

/// `Editops.as_opcodes().as_matching_blocks()`: `(src, dest, len)`.
fn matching_blocks(ops: &[Editop], len1: usize, len2: usize) -> Vec<(usize, usize, usize)> {
    let mut blocks = Vec::new();
    let mut equal = |s0: usize, s1: usize, d0: usize, d1: usize| {
        let length = (s1 - s0).min(d1 - d0);
        if length > 0 {
            blocks.push((s0, d0, length));
        }
    };
    let (mut src, mut dest) = (0, 0);
    let mut i = 0;
    while i < ops.len() {
        if src < ops[i].src || dest < ops[i].dest {
            equal(src, ops[i].src, dest, ops[i].dest);
            src = ops[i].src;
            dest = ops[i].dest;
        }
        let tag = ops[i].tag;
        while i < ops.len() && ops[i].tag == tag && src == ops[i].src && dest == ops[i].dest {
            match tag {
                Tag::Replace => {
                    src += 1;
                    dest += 1;
                }
                Tag::Insert => dest += 1,
                Tag::Delete => src += 1,
            }
            i += 1;
        }
    }
    if src < len1 || dest < len2 {
        equal(src, len1, dest, len2);
    }
    blocks.push((len1, len2, 0));
    blocks
}

/// `fuzz.ratio`.
pub fn ratio(s1: &str, s2: &str) -> i64 {
    if s1 == s2 {
        return 100;
    }
    if s1.is_empty() || s2.is_empty() {
        return 0;
    }
    let (a, b): (Vec<char>, Vec<char>) = (s1.chars().collect(), s2.chars().collect());
    intr(100.0 * indel_ratio(&a, &b))
}

/// `fuzz.partial_ratio`.
pub fn partial_ratio(s1: &str, s2: &str) -> i64 {
    if s1 == s2 {
        return 100;
    }
    if s1.is_empty() || s2.is_empty() {
        return 0;
    }
    let (a, b): (Vec<char>, Vec<char>) = (s1.chars().collect(), s2.chars().collect());
    let (shorter, longer) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let mut best = 0.0f64;
    for (src, dest, _) in matching_blocks(&editops(&shorter, &longer), shorter.len(), longer.len())
    {
        let start = dest.saturating_sub(src).min(longer.len());
        let end = (start + shorter.len()).min(longer.len());
        let r = indel_ratio(&shorter, &longer[start..end]);
        if r > 0.995 {
            return 100;
        }
        best = best.max(r);
    }
    intr(100.0 * best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rapidfuzz_examples() {
        // What RapidFuzz returns (its docstring says `delete s1[1]`, but both of
        // its implementations give 0).
        let ops = editops(&chars("qabxcd"), &chars("abycdf"));
        let got: Vec<(Tag, usize, usize)> = ops.iter().map(|o| (o.tag, o.src, o.dest)).collect();
        assert_eq!(
            got,
            vec![
                (Tag::Delete, 0, 0),
                (Tag::Replace, 3, 2),
                (Tag::Insert, 6, 5)
            ]
        );
        let blocks = matching_blocks(&editops(&chars("spam"), &chars("park")), 4, 4);
        assert_eq!(blocks, vec![(1, 0, 2), (4, 4, 0)]);
    }

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn scores() {
        assert_eq!(ratio("this is a test", "this is a test!"), 97);
        assert_eq!(partial_ratio("this is a test", "this is a test!"), 100);
        assert_eq!(partial_ratio("brown", "séra brown s01e01"), 100);
        assert_eq!(partial_ratio("", "x"), 0);
        assert_eq!(partial_ratio("", ""), 100);
    }
}
