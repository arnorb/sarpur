//! Finding the playlist to download, ported from ruvsarpur's
//! `find_m3u8_playlist_url`, `_playlist_quality_variants` and
//! `_playlist_default_audio_url` (R:506-674), and the worker's
//! `_resolve_playlist`.
//!
//! RÚV serves three shapes: current master playlists that advertise their
//! renditions (`1080p/stream.m3u8` with a `RESOLUTION`), and two older ones
//! where the rendition path is built from the quality's code or bitrate.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use super::http::{BROWSER_AGENT, Http};
use crate::settings::Quality;

pub const RUV_URL: &str = "https://ruv-vod.akamaized.net";

/// ruvsarpur's `QUALITY_BITRATE`, in its order.
const QUALITIES: [Quality; 3] = [Quality::Normal, Quality::HD720, Quality::HD1080];

fn code(q: Quality) -> &'static str {
    match q {
        Quality::Normal => "1200",
        Quality::HD720 => "2400",
        Quality::HD1080 => "3600",
    }
}

fn bits(q: Quality) -> &'static str {
    match q {
        Quality::Normal => "1150000",
        Quality::HD720 => "2350000",
        Quality::HD1080 => "3550000",
    }
}

static RESOLUTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)RESOLUTION=\d+x(?P<height>\d+)").unwrap());
static AUDIO_URI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)URI="(?P<uri>[^"]+)""#).unwrap());
static VOD_BASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?P<vodbase>.*)/(?P<rest>.*\.m3u8)").unwrap());

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Playlist {
    pub url: String,
    pub audio_url: Option<String>,
    pub fragments: usize,
    pub quality: Quality,
}

/// `urllib.parse.urljoin` for the references a playlist holds.
fn join(base: &str, reference: &str) -> String {
    match url::Url::parse(base).and_then(|b| b.join(reference)) {
        Ok(u) => u.to_string(),
        Err(_) => reference.to_string(),
    }
}

/// Python's `str.splitlines()` for playlist text.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split([
        '\n', '\r', '\u{0b}', '\u{0c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}',
        '\u{2029}',
    ])
}

/// `_playlist_quality_variants`: the largest rendition in each quality band.
pub fn quality_variants(master: &str, master_url: &str) -> HashMap<Quality, (String, u32)> {
    let mut variants: HashMap<Quality, (String, u32)> = HashMap::new();
    let mut pending: Option<u32> = None;
    for raw in lines(master) {
        let line = super::py::strip(raw);
        if line.starts_with("#EXT-X-STREAM-INF:") {
            pending = RESOLUTION
                .captures(line)
                .and_then(|c| c["height"].parse().ok());
        } else if let Some(height) = pending
            && !line.is_empty()
            && !line.starts_with('#')
        {
            let quality = if height >= 900 {
                Quality::HD1080
            } else if height >= 650 {
                Quality::HD720
            } else {
                Quality::Normal
            };
            if variants.get(&quality).is_none_or(|(_, h)| height > *h) {
                variants.insert(quality, (join(master_url, line), height));
            }
            pending = None;
        }
    }
    variants
}

/// `_playlist_default_audio_url`: the `DEFAULT=YES` audio track, else the first.
pub fn default_audio_url(master: &str, master_url: &str) -> Option<String> {
    let audio: Vec<&str> = lines(master)
        .map(super::py::strip)
        .filter(|l| l.starts_with("#EXT-X-MEDIA:TYPE=AUDIO"))
        .collect();
    let selected = audio
        .iter()
        .find(|l| l.to_uppercase().contains("DEFAULT=YES"))
        .or(audio.first())?;
    AUDIO_URI
        .captures(selected)
        .map(|c| join(master_url, &c["uri"]))
}

/// The requested quality first, then the smaller ones, then the larger.
fn candidates(requested: Quality) -> Vec<Quality> {
    let i = QUALITIES.iter().position(|q| *q == requested).unwrap_or(2);
    let mut out = vec![requested];
    out.extend(QUALITIES[..i].iter().rev());
    out.extend(&QUALITIES[i + 1..]);
    out
}

/// The media lines of a rendition playlist (`len(line) > 1`, not a tag).
fn fragments(text: &str) -> usize {
    lines(text)
        .filter(|l| l.chars().count() > 1 && !l.starts_with('#'))
        .count()
}

/// `getGroup(RE_VOD_BASE_URL, 'vodbase', url)`.
pub fn vod_base(url: &str) -> Option<String> {
    for line in url.split('\n') {
        // `.` doesn't cross lines, so each line is matched on its own.
        if let Some(c) = VOD_BASE.captures(line) {
            let base = super::py::strip(&c["vodbase"]);
            if !base.is_empty() {
                return Some(base.to_string());
            }
        }
    }
    None
}

async fn fetch(http: &Http, url: &str) -> Option<String> {
    match http.get(url, Some(BROWSER_AGENT)).await {
        Ok(r) if r.status == 200 && !r.body.is_empty() => Some(r.text()),
        Ok(r) => {
            tracing::debug!(url, status = r.status, "playlist not found");
            None
        }
        Err(e) => {
            tracing::debug!(url, error = %e, "playlist request failed");
            None
        }
    }
}

/// The worker's `_resolve_playlist`: the playlist for an item's `file`, in
/// the requested quality or the nearest one RÚV has. The error is the
/// message the download fails with.
pub async fn resolve(
    http: &Http,
    pid: &str,
    file: Option<&str>,
    quality: Quality,
) -> Result<Playlist, String> {
    let file = file.filter(|f| f.starts_with(RUV_URL)).ok_or_else(|| {
        format!(
            "pid {pid} has no {RUV_URL} video address (file={}); it can't be downloaded",
            file.map(|f| format!("'{f}'"))
                .unwrap_or_else(|| "None".into())
        )
    })?;
    let unresolved = || format!("could not resolve a playlist for pid {pid}");
    let vod = vod_base(file).ok_or_else(unresolved)?;
    let master = fetch(http, file).await.ok_or_else(unresolved)?;

    let variants = quality_variants(&master, file);
    if !variants.is_empty() {
        let audio_url = default_audio_url(&master, file);
        for q in candidates(quality) {
            let Some((url, _)) = variants.get(&q) else {
                continue;
            };
            if let Some(text) = fetch(http, url).await {
                return Ok(Playlist {
                    url: url.clone(),
                    audio_url,
                    fragments: fragments(&text),
                    quality: q,
                });
            }
        }
        return Err(unresolved());
    }

    let old_format = master.find(".m3u8?tlm=hls&streams").is_some_and(|i| i > 0);
    for q in candidates(quality) {
        let url = if old_format {
            format!("{vod}/asset-audio=50000-video={}.m3u8", bits(q))
        } else {
            format!("{vod}/{}/index.m3u8", code(q))
        };
        if let Some(text) = fetch(http, &url).await {
            return Ok(Playlist {
                url,
                audio_url: None,
                fragments: fragments(&text),
                quality: q,
            });
        }
    }
    Err(unresolved())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_order() {
        use Quality::*;
        assert_eq!(candidates(HD1080), vec![HD1080, HD720, Normal]);
        assert_eq!(candidates(HD720), vec![HD720, Normal, HD1080]);
        assert_eq!(candidates(Normal), vec![Normal, HD720, HD1080]);
    }

    #[test]
    fn variants_and_audio() {
        let master = "#EXTM3U\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",URI=\"audio/en.m3u8\"\n\
            #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",DEFAULT=YES,URI=\"audio/is.m3u8\"\n\
            #EXT-X-STREAM-INF:BANDWIDTH=1,RESOLUTION=1280x720\n720p/stream.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=2,RESOLUTION=1920x1080\n1080p/stream.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=3,RESOLUTION=640x360\nhttps://other/360.m3u8\n";
        let base = "https://ruv-vod.akamaized.net/opid/X/X.m3u8";
        let v = quality_variants(master, base);
        assert_eq!(
            v[&Quality::HD1080].0,
            "https://ruv-vod.akamaized.net/opid/X/1080p/stream.m3u8"
        );
        assert_eq!(v[&Quality::HD720].1, 720);
        assert_eq!(v[&Quality::Normal].0, "https://other/360.m3u8");
        assert_eq!(
            default_audio_url(master, base).as_deref(),
            Some("https://ruv-vod.akamaized.net/opid/X/audio/is.m3u8")
        );
        assert_eq!(
            vod_base(base).as_deref(),
            Some("https://ruv-vod.akamaized.net/opid/X")
        );
    }

    #[tokio::test]
    async fn resolves_recorded_playlists() {
        let http = Http::fixtures();
        // Current shape: advertised renditions.
        let p = resolve(
            &http,
            "p",
            Some("https://ruv-vod.akamaized.net/lokad/5499461-MRDLS2/5499461-MRDLS2.m3u8"),
            Quality::HD1080,
        )
        .await
        .unwrap();
        assert_eq!(
            p.url,
            "https://ruv-vod.akamaized.net/lokad/5499461-MRDLS2/1080p/stream.m3u8"
        );
        assert_eq!(p.quality, Quality::HD1080);
        assert!(p.fragments > 0);
        // Missing file address.
        let err = resolve(&http, "p", None, Quality::HD1080)
            .await
            .unwrap_err();
        assert!(
            err.contains("has no https://ruv-vod.akamaized.net video address"),
            "{err}"
        );
    }
}
