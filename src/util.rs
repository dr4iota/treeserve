use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn hexval(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_bytes(s: &str, plus_as_space: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hexval(b[i + 1]), hexval(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        if plus_as_space && b[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decode a URL path component ('+' stays literal).
pub fn percent_decode(s: &str) -> String {
    percent_decode_bytes(s, false)
}

/// Percent-encode a single path segment or query value.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Encode a slash-separated relative path for use in an href.
pub fn href_path(rel: &[String]) -> String {
    let mut out = String::from("/");
    for (i, seg) in rel.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(&percent_encode(seg));
    }
    out
}

/// A filesystem path as a human would write it.
///
/// `canonicalize` on Windows returns verbatim paths — `\\?\C:\dir`, or
/// `\\?\UNC\server\share` for a network location — and that prefix has no place
/// in a page or a title. For display only: the stored root keeps the verbatim
/// form, because `resolve_in_root` compares it against freshly canonicalized
/// paths and the two spellings would never match.
pub fn display_path(p: &Path) -> String {
    let s = p.display().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(unc) => format!(r"\\{unc}"),
            None => rest.to_string(),
        },
        None => s,
    }
}

/// Parse "a=1&b=2" into pairs; keys and values are decoded ('+' becomes space).
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for kv in q.split('&') {
        if kv.is_empty() {
            continue;
        }
        let (k, v) = match kv.split_once('=') {
            Some((k, v)) => (k, v),
            None => (kv, ""),
        };
        out.push((
            percent_decode_bytes(k, true),
            percent_decode_bytes(v, true),
        ));
    }
    out
}

pub fn query_get<'a>(q: &'a [(String, String)], key: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Parse a Cookie header value ("a=1; b=2") into pairs.
pub fn parse_cookies(header: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in header.split(';') {
        if let Some((k, v)) = part.trim().split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

/// Shell-style glob match: `*`, `?`, `[abc]`, `[a-z]`, `[!...]`.
/// Iterative with single-star backtracking; matches over chars.
pub fn fnmatch(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None; // (pattern idx after '*', text mark)

    while ti < t.len() {
        let step = if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi + 1, ti));
                    pi += 1;
                    continue;
                }
                '?' => Some(pi + 1),
                '[' => match match_class(&p, pi, t[ti]) {
                    Some((true, next)) => Some(next),
                    Some((false, _)) => None,
                    // unterminated class: treat '[' as a literal
                    None => (p[pi] == t[ti]).then_some(pi + 1),
                },
                c => (c == t[ti]).then_some(pi + 1),
            }
        } else {
            None
        };
        match step {
            Some(next) => {
                pi = next;
                ti += 1;
            }
            None => match star {
                Some((sp, mark)) => {
                    star = Some((sp, mark + 1));
                    pi = sp;
                    ti = mark + 1;
                }
                None => return false,
            },
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Parse a `[...]` class starting at p[start]=='['. Returns (matched, index after ']').
/// None if the class is unterminated.
fn match_class(p: &[char], start: usize, c: char) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = matches!(p.get(i), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let cur = *p.get(i)?;
        if cur == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|&e| e != ']') {
            let end = p[i + 2];
            if cur <= c && c <= end {
                matched = true;
            }
            i += 3;
        } else {
            if cur == c {
                matched = true;
            }
            i += 1;
        }
    }
}

pub fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" | "md" | "markdown" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tif" | "tiff" => "image/tiff",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

pub fn ext_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "bmp", "ico", "tif", "tiff",
];
pub const AUDIO_EXTS: &[&str] = &["mp3", "ogg", "oga", "opus", "wav", "flac", "m4a"];
pub const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "webm", "mkv", "mov", "ogv"];
pub const MARKDOWN_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd"];
pub const MERMAID_EXTS: &[&str] = &["mmd", "mermaid"];

pub fn human_size(n: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} B", n)
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

/// Format a mtime as "YYYY-MM-DD HH:MM" (UTC).
pub fn fmt_time(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60
    )
}

// Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Heuristic: a buffer is binary if it contains a NUL byte.
pub fn looks_binary(buf: &[u8]) -> bool {
    buf.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basics() {
        assert!(fnmatch("*.rs", "main.rs"));
        assert!(!fnmatch("*.rs", "main.rc"));
        assert!(fnmatch("a?c", "abc"));
        assert!(fnmatch("[a-c]x", "bx"));
        assert!(!fnmatch("[!a-c]x", "bx"));
        assert!(fnmatch("*", "anything"));
        assert!(fnmatch("foo*bar*baz", "foo_bar__baz"));
        assert!(!fnmatch("foo*bar", "foo_baz"));
        assert!(fnmatch("[", "[")); // unterminated class is literal
    }

    #[test]
    fn percent_roundtrip() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn civil() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19723), (2024, 1, 1));
    }
}
