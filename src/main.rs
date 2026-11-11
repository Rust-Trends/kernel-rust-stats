//! kernel-rust-stats: measures Rust in the Linux kernel, release by release.

mod analyze;
mod areas;
mod fetch;
mod loc;
mod makefile;
mod model;
mod visit;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fetch::Tag;
use model::*;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const USAGE: &str = "\
kernel-rust-stats: measure Rust in the Linux kernel and write linux-rust.json

USAGE:
    kernel-rust-stats run      [--tags v6.18,v6.19] [--out linux-rust.json] [--cache-dir cache]
                               [--jobs 3] [--refresh] [--cross-checks cross_checks.json]
    kernel-rust-stats assemble [--out linux-rust.json] [--cache-dir cache]
                               [--cross-checks cross_checks.json]

run       Analyze every final release from v6.1 on (plus the newest -rc if newer), or the
          --tags given. Results are cached per tag in <cache-dir>/<tag>.json; cached tags are
          skipped unless --refresh. Then runs assemble.
assemble  Combine the cached per-tag results into the output JSON (no network).
";

struct Opts {
    tags: Option<Vec<String>>,
    out: PathBuf,
    cache_dir: PathBuf,
    jobs: usize,
    refresh: bool,
    cross_checks: PathBuf,
}

fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut o = Opts {
        tags: None,
        out: "linux-rust.json".into(),
        cache_dir: "cache".into(),
        jobs: 3,
        refresh: false,
        cross_checks: "cross_checks.json".into(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--tags" => o.tags = Some(val()?.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()),
            "--out" => o.out = val()?.into(),
            "--cache-dir" => o.cache_dir = val()?.into(),
            "--jobs" => o.jobs = val()?.parse().map_err(|_| "--jobs needs a number")?,
            "--cross-checks" => o.cross_checks = val()?.into(),
            "--refresh" => o.refresh = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}").into()),
        }
    }
    o.jobs = o.jobs.clamp(1, 8);
    Ok(o)
}

fn cache_path(dir: &Path, tag: &str) -> PathBuf {
    dir.join(format!("{tag}.json"))
}

fn run(o: &Opts) -> Result<()> {
    let tags: Vec<Tag> = match &o.tags {
        Some(list) => list
            .iter()
            .map(|s| Tag::parse(s).ok_or_else(|| format!("not a kernel tag: {s}")))
            .collect::<std::result::Result<_, _>>()?,
        None => fetch::discover_tags()?,
    };
    std::fs::create_dir_all(&o.cache_dir)?;
    let todo: Vec<Tag> = tags
        .iter()
        .filter(|t| o.refresh || !cache_path(&o.cache_dir, &t.name).exists())
        .cloned()
        .collect();
    eprintln!(
        "{} tags ({}), {} to analyze with {} jobs",
        tags.len(),
        tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(" "),
        todo.len(),
        o.jobs
    );

    let queue = Arc::new(Mutex::new(todo.into_iter().rev().collect::<Vec<_>>()));
    let failures = Arc::new(Mutex::new(Vec::<String>::new()));
    std::thread::scope(|s| {
        for _ in 0..o.jobs {
            let queue = Arc::clone(&queue);
            let failures = Arc::clone(&failures);
            let cache_dir = o.cache_dir.clone();
            // syn on deeply nested files needs more than the default 2 MiB thread stack.
            std::thread::Builder::new()
                .stack_size(64 << 20)
                .spawn_scoped(s, move || loop {
                    let Some(tag) = queue.lock().unwrap().pop() else { break };
                    let res = analyze::analyze_tag(&tag).and_then(|rel| {
                        let json = serde_json::to_string_pretty(&rel)?;
                        let path = cache_path(&cache_dir, &tag.name);
                        let tmp = path.with_extension("json.tmp");
                        std::fs::write(&tmp, json + "\n")?;
                        std::fs::rename(&tmp, &path)?;
                        Ok(())
                    });
                    if let Err(e) = res {
                        eprintln!("[{}] FAILED: {e}", tag.name);
                        failures.lock().unwrap().push(tag.name.clone());
                    }
                })
                .expect("spawn worker");
        }
    });
    let failed = failures.lock().unwrap().clone();
    if !failed.is_empty() {
        return Err(format!("failed tags: {}", failed.join(", ")).into());
    }
    assemble(o)
}

fn load_cache(dir: &Path) -> Result<Vec<Release>> {
    let mut by_name: BTreeMap<String, Release> = BTreeMap::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))? {
        let p = e?.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let rel: Release = serde_json::from_str(&std::fs::read_to_string(&p)?)
            .map_err(|e| format!("parsing {}: {e}", p.display()))?;
        by_name.insert(rel.tag.clone(), rel);
    }
    // Same selection rule as discovery: finals plus the newest rc if it is newer.
    let names: Vec<&str> = by_name.keys().map(|s| s.as_str()).collect();
    let selected = fetch::select_tags(&names);
    Ok(selected.into_iter().filter_map(|t| by_name.remove(&t.name)).collect())
}

pub fn build_output(releases: Vec<Release>, cross_checks: Vec<CrossCheck>, now: u64) -> Output {
    // Component timeline.
    let mut comps: BTreeMap<String, (usize, ComponentSpan)> = BTreeMap::new();
    for (i, r) in releases.iter().enumerate() {
        for c in &r.components {
            let e = comps.entry(c.id.clone()).or_insert_with(|| {
                (i, ComponentSpan {
                    id: c.id.clone(),
                    label: String::new(),
                    path: String::new(),
                    area: String::new(),
                    first_tag: r.tag.clone(),
                    last_tag: String::new(),
                    latest_code: 0,
                })
            });
            e.1.label = c.label.clone();
            e.1.path = c.path.clone();
            e.1.area = c.area.clone();
            e.1.last_tag = r.tag.clone();
            e.1.latest_code = c.code;
        }
    }
    let mut component_timeline: Vec<(usize, ComponentSpan)> = comps.into_values().collect();
    component_timeline.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id)));

    let mut crates: BTreeMap<String, (usize, CrateSpan)> = BTreeMap::new();
    for (i, r) in releases.iter().enumerate() {
        for c in &r.crates {
            let e = crates.entry(c.name.clone()).or_insert_with(|| {
                (i, CrateSpan { name: c.name.clone(), kind: String::new(), first_tag: r.tag.clone(), last_tag: String::new() })
            });
            e.1.kind = c.kind.clone();
            e.1.last_tag = r.tag.clone();
        }
    }
    let mut crate_timeline: Vec<(usize, CrateSpan)> = crates.into_values().collect();
    crate_timeline.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));

    Output {
        schema_version: 1,
        generated_at: fetch::rfc3339(now),
        source: "https://github.com/torvalds/linux".into(),
        analyzer: Analyzer {
            name: env!("CARGO_PKG_NAME").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            repo: "https://github.com/Rust-Trends/kernel-rust-stats".into(),
        },
        areas: areas::AREAS.iter().map(|(id, label)| AreaDef { id: id.to_string(), label: label.to_string() }).collect(),
        releases,
        component_timeline: component_timeline.into_iter().map(|x| x.1).collect(),
        crate_timeline: crate_timeline.into_iter().map(|x| x.1).collect(),
        cross_checks,
    }
}

fn assemble(o: &Opts) -> Result<()> {
    let releases = load_cache(&o.cache_dir)?;
    if releases.is_empty() {
        return Err(format!("no cached releases in {}", o.cache_dir.display()).into());
    }
    let cross_checks: Vec<CrossCheck> = if o.cross_checks.exists() {
        serde_json::from_str(&std::fs::read_to_string(&o.cross_checks)?)
            .map_err(|e| format!("parsing {}: {e}", o.cross_checks.display()))?
    } else {
        eprintln!("warning: {} not found, cross_checks will be empty", o.cross_checks.display());
        vec![]
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let n = releases.len();
    let out = build_output(releases, cross_checks, now);
    std::fs::write(&o.out, serde_json::to_string_pretty(&out)? + "\n")?;
    eprintln!("wrote {} ({} releases, newest {})", o.out.display(), n, out.releases[n - 1].tag);
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = match args.first().map(|s| s.as_str()) {
        Some("run") => parse_opts(&args[1..]).and_then(|o| run(&o)),
        Some("assemble") => parse_opts(&args[1..]).and_then(|o| assemble(&o)),
        Some("-h") | Some("--help") => {
            print!("{USAGE}");
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
