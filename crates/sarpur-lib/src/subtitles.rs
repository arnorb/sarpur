//! WebVTT to SRT, ported from ruvsarpur's `convertVttSubtitleFileToSrt`
//! (R:382). The output must match the Python function byte for byte; the
//! fixtures in `fixtures/subtitles/` are checked against both.

/// Python's `str.splitlines()`: splits on `\n`, `\r\n`, `\r`, `\v`, `\f`,
/// `\x1c`–`\x1e`, `\x85`, ` ` and ` `, with no trailing empty line.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let is_break = matches!(
            c,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !is_break {
            continue;
        }
        lines.push(&text[start..i]);
        let mut end = i + c.len_utf8();
        if c == '\r' {
            if let Some(&(j, '\n')) = chars.peek() {
                chars.next();
                end = j + 1;
            }
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Python's `str.strip()` with no arguments (Unicode whitespace).
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

fn digits(s: &str, n: usize) -> Option<&str> {
    let head = s.get(..n)?;
    head.bytes().all(|b| b.is_ascii_digit()).then(|| &s[n..])
}

/// `\d{2}:\d{2}\.\d{3}` at the start of `s`; returns the rest.
fn mm_ss_ms(s: &str) -> Option<&str> {
    let s = digits(s, 2)?.strip_prefix(':')?;
    let s = digits(s, 2)?.strip_prefix('.')?;
    digits(s, 3)
}

/// `(?:\d{2}:)?\d{2}:\d{2}\.\d{3}` at the start of `s`, as a regex would
/// match it (with the optional hours first). Returns the match and the rest.
fn timestamp(s: &str) -> Option<(&str, &str)> {
    let with_hours = digits(s, 2)
        .and_then(|r| r.strip_prefix(':'))
        .and_then(mm_ss_ms);
    let rest = with_hours.or_else(|| mm_ss_ms(s))?;
    Some((&s[..s.len() - rest.len()], rest))
}

/// `RE_SUBTITLE_TIMECODE.match(line)`: start and end timestamps.
fn timecode(line: &str) -> Option<(&str, &str)> {
    let (start, rest) = timestamp(line)?;
    let rest = rest.trim_start_matches(char::is_whitespace);
    let rest = rest.strip_prefix("-->")?;
    let rest = rest.trim_start_matches(char::is_whitespace);
    let (end, _) = timestamp(rest)?;
    Some((start, end))
}

/// `formatSrtTimestamp`: adds missing hours and uses a decimal comma.
fn srt_timestamp(value: &str) -> String {
    let value = py_strip(value);
    let value = if value.matches(':').count() == 1 {
        format!("00:{value}")
    } else {
        value.to_string()
    };
    value.replace('.', ",")
}

/// Converts WebVTT text to SRT. `None` when there are no cues (ruvsarpur then
/// writes no `.srt`).
pub fn vtt_to_srt(vtt: &str) -> Option<String> {
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    let mut timecode_now: Option<String> = None;
    let mut text_lines: Vec<String> = Vec::new();
    let mut skip_metadata_block = false;

    for raw_line in split_lines(vtt) {
        let clean_line = raw_line.replace('\u{feff}', "");
        let stripped = py_strip(&clean_line);

        if skip_metadata_block {
            if stripped.is_empty() {
                skip_metadata_block = false;
            }
            continue;
        }
        if stripped.is_empty() {
            if let Some(tc) = timecode_now.take() {
                entries.push((tc, std::mem::take(&mut text_lines)));
            }
            text_lines.clear();
            continue;
        }
        if stripped.to_uppercase() == "WEBVTT" {
            continue;
        }
        if stripped.starts_with("NOTE") || stripped == "STYLE" || stripped == "REGION" {
            skip_metadata_block = true;
            continue;
        }
        if let Some((start, end)) = timecode(stripped) {
            timecode_now = Some(format!(
                "{} --> {}",
                srt_timestamp(start),
                srt_timestamp(end)
            ));
            continue;
        }
        if timecode_now.is_none() {
            // Cue identifiers and unsupported directives.
            continue;
        }
        text_lines.push(clean_line);
    }
    if let Some(tc) = timecode_now {
        entries.push((tc, text_lines));
    }
    if entries.is_empty() {
        return None;
    }
    let mut out = String::new();
    for (index, (tc, lines)) in entries.iter().enumerate() {
        out.push_str(&format!("{}\n{tc}\n", index + 1));
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn splits_like_python() {
        assert_eq!(split_lines("a\r\nb\rc\nd\n"), ["a", "b", "c", "d"]);
        assert_eq!(split_lines("a\n\nb"), ["a", "", "b"]);
        assert_eq!(split_lines("a\u{2028}b\u{85}c"), ["a", "b", "c"]);
        assert!(split_lines("").is_empty());
    }

    #[test]
    fn timecodes() {
        assert_eq!(
            timecode("00:01:07.000 --> 00:01:12.040 line:10 align:middle"),
            Some(("00:01:07.000", "00:01:12.040"))
        );
        assert_eq!(
            timecode("01:07.000-->01:12.040"),
            Some(("01:07.000", "01:12.040"))
        );
        assert_eq!(timecode("1:07.000 --> 01:12.040"), None);
        assert_eq!(srt_timestamp("01:07.000"), "00:01:07,000");
    }

    /// Criterion 14: every `fixtures/subtitles/*.vtt` converts to the `.srt`
    /// next to it, which `worker/test_subtitles.py` checks against R:382.
    #[test]
    fn matches_ruvsarpur_on_fixtures() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/subtitles");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "vtt") {
                continue;
            }
            let vtt = std::fs::read_to_string(&path).unwrap();
            let expected = path.with_extension("srt");
            let got = vtt_to_srt(&vtt);
            if expected.exists() {
                let want = std::fs::read_to_string(&expected).unwrap();
                assert_eq!(got.as_deref(), Some(want.as_str()), "{}", path.display());
            } else {
                assert_eq!(got, None, "{} has no cues", path.display());
            }
            checked += 1;
        }
        assert!(checked >= 4, "only {checked} fixtures");
    }
}
