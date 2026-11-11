//! Tag discovery and streaming tarball access. Nothing is written to disk: `curl` streams the
//! tarball into the decompressor, the decompressor into the `tar` reader, and each entry is
//! handed to a callback as bytes in memory.

use std::io::Read;
use std::process::{Child, Command, Stdio};

use crate::Result;

/// A kernel tag such as `v6.19` or `v7.4-rc2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub major: u32,
    pub minor: u32,
    /// `None` for a final release.
    pub rc: Option<u32>,
}

impl Tag {
    pub fn parse(s: &str) -> Option<Tag> {
        let rest = s.strip_prefix('v')?;
        let (ver, rc) = match rest.split_once("-rc") {
            Some((v, r)) => (v, Some(r.parse().ok()?)),
            None => (rest, None),
        };
        let (maj, min) = ver.split_once('.')?;
        if !(maj.chars().all(|c| c.is_ascii_digit()) && min.chars().all(|c| c.is_ascii_digit())) {
            return None;
        }
        Some(Tag { name: s.to_string(), major: maj.parse().ok()?, minor: min.parse().ok()?, rc })
    }

    pub fn version(&self) -> String {
        match self.rc {
            Some(rc) => format!("{}.{}-rc{}", self.major, self.minor, rc),
            None => format!("{}.{}", self.major, self.minor),
        }
    }

    /// Sort key: rcs of X.Y come before the X.Y final.
    pub fn key(&self) -> (u32, u32, u32) {
        (self.major, self.minor, self.rc.unwrap_or(u32::MAX))
    }

    pub fn url(&self) -> String {
        match self.rc {
            None => format!(
                "https://cdn.kernel.org/pub/linux/kernel/v{}.x/linux-{}.{}.tar.xz",
                self.major, self.major, self.minor
            ),
            Some(_) => format!("https://git.kernel.org/torvalds/t/linux-{}.tar.gz", self.version()),
        }
    }

    pub fn fallback_url(&self) -> String {
        format!("https://codeload.github.com/torvalds/linux/tar.gz/refs/tags/{}", self.name)
    }
}

/// Every final from v6.1 on, plus the newest rc if it is newer than the newest final.
pub fn select_tags(all: &[&str]) -> Vec<Tag> {
    let mut tags: Vec<Tag> = all
        .iter()
        .filter_map(|s| Tag::parse(s))
        .filter(|t| (6..=8).contains(&t.major))
        .filter(|t| t.key() >= (6, 1, 0))
        .collect();
    tags.sort_by_key(|t| t.key());
    tags.dedup_by_key(|t| t.key());
    let newest_rc = tags.iter().filter(|t| t.rc.is_some()).last().cloned();
    let newest_final = tags.iter().filter(|t| t.rc.is_none()).last().cloned();
    let mut out: Vec<Tag> = tags.into_iter().filter(|t| t.rc.is_none() && t.key() >= (6, 1, u32::MAX)).collect();
    if let Some(rc) = newest_rc {
        if newest_final.map_or(true, |f| rc.key() > f.key()) {
            out.push(rc);
        }
    }
    out
}

pub fn discover_tags() -> Result<Vec<Tag>> {
    let out = Command::new("git")
        .args(["ls-remote", "--tags", "https://github.com/torvalds/linux"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("running git ls-remote: {e}"))?;
    if !out.status.success() {
        return Err("git ls-remote failed".into());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let names: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split('\t').nth(1))
        .filter_map(|r| r.strip_prefix("refs/tags/"))
        .filter(|r| !r.ends_with("^{}"))
        .collect();
    let tags = select_tags(&names);
    if tags.is_empty() {
        return Err("no kernel tags found".into());
    }
    Ok(tags)
}

fn url_ok(url: &str) -> bool {
    Command::new("curl")
        .args(["-sfIL", "-o", "/dev/null", "--max-time", "60", url])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Picks a working URL for the tag.
pub fn resolve_url(tag: &Tag) -> Result<String> {
    let primary = tag.url();
    if url_ok(&primary) {
        return Ok(primary);
    }
    let fb = tag.fallback_url();
    eprintln!("[{}] {} not reachable, trying {}", tag.name, primary, fb);
    if url_ok(&fb) {
        return Ok(fb);
    }
    Err(format!("no working tarball URL for {}", tag.name).into())
}

/// Kills the curl/xz children if the reader is dropped early.
struct Pipeline {
    children: Vec<Child>,
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Streams `url` and calls `f(path_without_top_dir, mtime, bytes)` for every regular file for
/// which `want(path)` is true. Returns the largest entry mtime seen.
pub fn for_each_entry(
    url: &str,
    want: impl Fn(&str) -> bool,
    mut f: impl FnMut(&str, &[u8]) -> Result<()>,
) -> Result<u64> {
    let mut curl = Command::new("curl")
        .args(["-sSfL", "--retry", "3", "--retry-delay", "5", "--connect-timeout", "30", url])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawning curl: {e}"))?;
    let body = curl.stdout.take().ok_or("no curl stdout")?;
    let mut pipeline = Pipeline { children: vec![curl] };

    let reader: Box<dyn Read> = if url.ends_with(".xz") {
        Box::new(xz2::read::XzDecoder::new_multi_decoder(body))
    } else {
        Box::new(flate2::read::MultiGzDecoder::new(body))
    };
    let mut archive = tar::Archive::new(reader);
    let mut max_mtime = 0u64;
    let mut buf = Vec::new();
    for entry in archive.entries().map_err(|e| format!("tar: {e}"))? {
        let mut entry = entry.map_err(|e| format!("tar entry: {e}"))?;
        if entry.header().entry_type() != tar::EntryType::Regular {
            continue;
        }
        let full = entry.path().map_err(|e| format!("tar path: {e}"))?.to_string_lossy().into_owned();
        let Some((_, rel)) = full.split_once('/') else { continue };
        if let Ok(m) = entry.header().mtime() {
            max_mtime = max_mtime.max(m);
        }
        if !want(rel) {
            continue;
        }
        buf.clear();
        entry.read_to_end(&mut buf).map_err(|e| format!("reading {rel}: {e}"))?;
        f(rel, &buf)?;
    }
    // Drain to EOF so curl exits cleanly, then check its status.
    let mut rest = archive.into_inner();
    std::io::copy(&mut rest, &mut std::io::sink()).ok();
    let mut curl = pipeline.children.pop().unwrap();
    let status = curl.wait().map_err(|e| format!("waiting for curl: {e}"))?;
    if !status.success() {
        return Err(format!("curl failed for {url}: {status}").into());
    }
    if max_mtime == 0 {
        return Err(format!("empty archive from {url}").into());
    }
    Ok(max_mtime)
}

/// Unix seconds -> `YYYY-MM-DD` (UTC).
pub fn ymd(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Unix seconds -> RFC 3339 UTC.
pub fn rfc3339(secs: u64) -> String {
    let rem = secs % 86_400;
    format!("{}T{:02}:{:02}:{:02}Z", ymd(secs), rem / 3600, (rem % 3600) / 60, rem % 60)
}

// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_selection() {
        let all = [
            "v6.0", "v6.1-rc1", "v6.1", "v6.2", "v6.19", "v7.0-rc1", "v7.0", "v7.4-rc1", "v7.4-rc2", "v5.19",
            "v6.2^{}", "v2.6.11",
        ];
        let got: Vec<String> = select_tags(&all).into_iter().map(|t| t.name).collect();
        assert_eq!(got, vec!["v6.1", "v6.2", "v6.19", "v7.0", "v7.4-rc2"]);
        let all = ["v6.1", "v7.3-rc7", "v7.3"];
        let got: Vec<String> = select_tags(&all).into_iter().map(|t| t.name).collect();
        assert_eq!(got, vec!["v6.1", "v7.3"]);
    }

    #[test]
    fn urls() {
        let t = Tag::parse("v6.19").unwrap();
        assert_eq!(t.url(), "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.19.tar.xz");
        let t = Tag::parse("v7.4-rc2").unwrap();
        assert_eq!(t.url(), "https://git.kernel.org/torvalds/t/linux-7.4-rc2.tar.gz");
        assert_eq!(t.version(), "7.4-rc2");
    }

    #[test]
    fn dates() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(1_770_508_800), "2026-02-08");
        assert_eq!(rfc3339(1_770_552_000), "2026-02-08T12:00:00Z");
    }
}
