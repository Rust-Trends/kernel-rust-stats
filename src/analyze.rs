//! Analysis of one release: streams the tarball once and builds a `Release`.

use std::collections::{BTreeMap, BTreeSet};

use crate::areas::{self, Area};
use crate::loc::{self as lines, Lang};
use crate::fetch::{self, Tag};
use crate::makefile;
use crate::model::*;
use crate::visit::{self, Items, Unsafe};
use crate::Result;

const TOP_N: usize = 25;

/// A `.rs` file as remembered after its bytes are dropped.
#[derive(Debug, Clone)]
pub struct RsFile {
    pub path: String,
    pub area: Area,
    pub code: u64,
}

/// Accumulates per-file results; independent of where the files come from (tests feed it directly).
#[derive(Default)]
pub struct Accumulator {
    pub rust: ByArea<Loc>,
    pub c: Loc,
    pub h: Loc,
    pub items: ByArea<Items>,
    pub unsafe_: ByArea<Unsafe>,
    pub ffi: BTreeMap<String, u64>,
    pub kernel_imports: BTreeMap<String, u64>,
    pub rs_files: Vec<RsFile>,
    /// Directories under drivers/ that hold at least one C file.
    pub c_dirs: BTreeSet<String>,
    pub rust_makefile: Option<String>,
}


pub fn wanted(path: &str) -> bool {
    path.ends_with(".rs") || path.ends_with(".c") || path.ends_with(".h") || path == "rust/Makefile"
}

impl Accumulator {
    pub fn add_file(&mut self, path: &str, bytes: &[u8]) {
        if path == "rust/Makefile" {
            self.rust_makefile = Some(String::from_utf8_lossy(bytes).into_owned());
        } else if path.ends_with(".c") {
            self.c.add(&lines::count(Lang::C, bytes));
            if path.starts_with("drivers/") {
                self.c_dirs.insert(areas::parent_dir(path).to_string());
            }
        } else if path.ends_with(".h") {
            self.h.add(&lines::count(Lang::C, bytes));
        } else if path.ends_with(".rs") {
            let area = areas::classify(path);
            let loc = lines::count(Lang::Rust, bytes);
            self.rust.get_mut(area).add(&loc);
            self.rs_files.push(RsFile { path: path.to_string(), area, code: loc.code });

            let src = String::from_utf8_lossy(bytes);
            let facts = visit::analyze_source(&src);
            if facts.items.parse_errors > 0 {
                eprintln!("parse error (skipped): {path}");
            }
            self.items.get_mut(area).add(&facts.items);
            self.unsafe_.get_mut(area).add(&facts.unsafe_);
            if area.is_first_party() {
                for (k, v) in facts.ffi_calls {
                    *self.ffi.entry(k).or_default() += v;
                }
            }
            if !matches!(area, Area::Core | Area::Vendored) {
                for m in facts.kernel_imports {
                    *self.kernel_imports.entry(m).or_default() += 1;
                }
            }
        }
    }

    pub fn components(&self, log_tag: &str) -> Vec<Component> {
        struct Acc {
            label: String,
            path: String,
            area: Area,
            files: u64,
            code: u64,
        }
        let mut by_id: BTreeMap<String, Acc> = BTreeMap::new();
        let mut unmatched: Vec<&RsFile> = Vec::new();
        for f in &self.rs_files {
            if let Some((def, p)) = areas::match_component(&f.path) {
                let path = if p.ends_with('/') { p.to_string() } else { f.path.clone() };
                let e = by_id.entry(def.id.to_string()).or_insert(Acc {
                    label: def.label.to_string(),
                    path,
                    area: f.area,
                    files: 0,
                    code: 0,
                });
                e.files += 1;
                e.code += f.code;
            } else if f.area == Area::Drivers {
                unmatched.push(f);
            }
        }

        // Fallback: unmatched driver files. A directory with several Rust files and no C files
        // becomes one component; otherwise each file is its own component.
        let mut per_dir: BTreeMap<&str, Vec<&RsFile>> = BTreeMap::new();
        for f in &unmatched {
            per_dir.entry(areas::parent_dir(&f.path)).or_default().push(f);
        }
        for (dir, files) in per_dir {
            let as_dir = files.len() > 1 && !self.c_dirs.contains(dir);
            let groups: Vec<(String, String, String, Vec<&RsFile>)> = if as_dir {
                let name = dir.trim_end_matches('/').rsplit('/').next().unwrap_or(dir).to_string();
                vec![(name.replace('_', "-"), format!("{name} (Rust driver)"), dir.to_string(), files)]
            } else {
                files
                    .into_iter()
                    .map(|f| {
                        let stem = areas::file_stem(&f.path).to_string();
                        (stem.replace('_', "-"), format!("{stem} (Rust driver)"), f.path.clone(), vec![f])
                    })
                    .collect()
            };
            for (id, label, path, files) in groups {
                eprintln!("[{log_tag}] unmatched component: {id} -> {path}");
                let e = by_id.entry(id).or_insert(Acc { label, path, area: Area::Drivers, files: 0, code: 0 });
                for f in files {
                    e.files += 1;
                    e.code += f.code;
                }
            }
        }

        let mut out: Vec<Component> = by_id
            .into_iter()
            .map(|(id, a)| Component {
                id,
                label: a.label,
                path: a.path,
                area: a.area.id().to_string(),
                files: a.files,
                code: a.code,
            })
            .collect();
        out.sort_by(|a, b| b.code.cmp(&a.code).then(a.id.cmp(&b.id)));
        out
    }

    pub fn crates(&self, log_tag: &str) -> Vec<Crate> {
        let Some(mk) = &self.rust_makefile else {
            eprintln!("[{log_tag}] warning: no rust/Makefile");
            return vec![];
        };
        makefile::parse(mk)
            .into_iter()
            .map(|c| {
                let code = if c.kind == "toolchain" {
                    0
                } else if c.path.ends_with('/') {
                    self.rs_files.iter().filter(|f| f.path.starts_with(&c.path)).map(|f| f.code).sum()
                } else {
                    self.rs_files.iter().filter(|f| f.path == c.path).map(|f| f.code).sum()
                };
                Crate { name: c.name, kind: c.kind.to_string(), path: c.path, code, deps: c.deps }
            })
            .collect()
    }

    pub fn finish(self, tag: &Tag, mtime: u64) -> Release {
        let mut rust = Loc::default();
        let mut items = Items::default();
        let mut unsafe_ = Unsafe::default();
        for a in Area::ALL {
            rust.add(self.rust.get(a));
            items.add(self.items.get(a));
            unsafe_.add(self.unsafe_.get(a));
        }
        let c_code = (self.c.code + self.h.code) as f64;
        let share = |r: u64| if r as f64 + c_code > 0.0 { r as f64 / (r as f64 + c_code) } else { 0.0 };
        let fp = rust.code - self.rust.vendored.code;

        let mut ffi_top: Vec<NameCalls> =
            self.ffi.iter().map(|(k, v)| NameCalls { name: k.clone(), calls: *v }).collect();
        ffi_top.sort_by(|a, b| b.calls.cmp(&a.calls).then(a.name.cmp(&b.name)));
        let call_sites = ffi_top.iter().map(|x| x.calls).sum();
        let distinct = ffi_top.len() as u64;
        ffi_top.truncate(TOP_N);

        let mut imp: Vec<ModuleFiles> =
            self.kernel_imports.iter().map(|(k, v)| ModuleFiles { module: k.clone(), files: *v }).collect();
        imp.sort_by(|a, b| b.files.cmp(&a.files).then(a.module.cmp(&b.module)));
        imp.truncate(TOP_N);

        let components = self.components(&tag.name);
        let crates = self.crates(&tag.name);

        Release {
            tag: tag.name.clone(),
            version: tag.version(),
            is_rc: tag.rc.is_some(),
            date: fetch::ymd(mtime),
            loc: LocBlock { rust, rust_by_area: self.rust, c: self.c, h: self.h },
            rust_share: round6(share(rust.code)),
            rust_share_first_party: round6(share(fp)),
            items,
            items_by_area: self.items,
            unsafe_,
            unsafe_by_area: self.unsafe_,
            ffi: Ffi { call_sites, distinct_functions: distinct, top: ffi_top },
            imports: Imports { kernel_top: imp },
            components,
            crates,
        }
    }
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

pub fn analyze_tag(tag: &Tag) -> Result<Release> {
    let url = fetch::resolve_url(tag)?;
    eprintln!("[{}] streaming {}", tag.name, url);
    let mut acc = Accumulator::default();
    let mut n = 0u64;
    let started = std::time::Instant::now();
    let mtime = fetch::for_each_entry(&url, wanted, |path, bytes| {
        acc.add_file(path, bytes);
        n += 1;
        if n % 10_000 == 0 {
            eprintln!("[{}] {} files ({:.0}s)", tag.name, n, started.elapsed().as_secs_f64());
        }
        Ok(())
    })?;
    let rel = acc.finish(tag, mtime);
    eprintln!(
        "[{}] done in {:.0}s: {} rs files, rust code {}, c+h code {}, parse errors {}",
        tag.name,
        started.elapsed().as_secs_f64(),
        rel.loc.rust.files,
        rel.loc.rust.code,
        rel.loc.c.code + rel.loc.h.code,
        rel.items.parse_errors
    );
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_and_components() {
            let mut acc = Accumulator::default();
        let rs = b"use kernel::prelude::*;\nfn f() {\n    unsafe { bindings::foo() };\n}\n";
        acc.add_file("drivers/net/phy/qt2025.rs", rs);
        acc.add_file("drivers/foo/a.rs", rs);
        acc.add_file("drivers/foo/b.rs", rs);
        acc.add_file("drivers/bar/x_rust.rs", rs);
        acc.add_file("drivers/bar/y.c", b"int y;\n");
        acc.add_file("drivers/bar/z_rust.rs", rs);
        acc.add_file("rust/syn/lib.rs", rs);
        acc.add_file("rust/kernel/lib.rs", rs);
        acc.add_file("include/x.h", b"/* c */\nint x;\n\n");
        let tag = Tag::parse("v6.19").unwrap();
        let r = acc.finish(&tag, 0);
        assert_eq!(r.loc.rust.files, 7);
        assert_eq!(r.loc.rust.code, 28);
        assert_eq!(r.loc.rust_by_area.vendored.code, 4);
        assert_eq!(r.loc.c.code, 1);
        assert_eq!(r.loc.h.code, 1);
        assert_eq!(r.loc.h.comments, 1);
        assert_eq!(r.loc.h.blanks, 1);
        // FFI: first party only (6 of 7 files).
        assert_eq!(r.ffi.call_sites, 6);
        // kernel imports: files outside rust/ only.
        assert_eq!(r.imports.kernel_top[0].files, 5);
        let ids: Vec<&str> = r.components.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["foo", "qt2025", "x-rust", "z-rust"]);
        let foo = &r.components[0];
        assert_eq!((foo.path.as_str(), foo.files, foo.code), ("drivers/foo/", 2, 8));
        assert!((r.rust_share - 28.0 / 30.0).abs() < 1e-5);
        assert!((r.rust_share_first_party - 24.0 / 26.0).abs() < 1e-5);
    }
}
