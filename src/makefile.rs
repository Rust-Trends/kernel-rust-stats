//! Best-effort crate graph from `rust/Makefile`.
//!
//! The Makefile changed a lot between v6.1 and v7.x, so this does not evaluate Make. It looks
//! for build rules `$(obj)/<target>: <first prerequisite> ...` whose first prerequisite is a Rust
//! source file, and for target-specific `rustc_target_flags` assignments, expanding `$(var)`
//! references to variables defined in the same file, to find `--extern <crate>` dependencies.
//!
//! Target name -> crate name:
//! * `X.o`                         -> `X`
//! * `libX.rlib`, `host/libX.rlib` -> `X`      (host libraries for proc macros)
//! * `libX.so`                     -> `X`      (v6.1-era proc macro output)
//! * `$(libX_name)`                -> `X`      (later proc macro output)

use std::collections::{BTreeMap, BTreeSet};

use crate::areas::VENDORED_DIRS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateDef {
    pub name: String,
    /// first_party | vendored | toolchain
    pub kind: &'static str,
    /// Tree-relative directory (with trailing `/`) or file; empty for toolchain crates.
    pub path: String,
    pub deps: Vec<String>,
}

/// Joins backslash-continued lines and strips comments.
fn logical_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for raw in src.lines() {
        let line = match raw.find('#') {
            // `#` inside recipes or strings is rare in the rules we care about; good enough.
            Some(i) if !raw[..i].ends_with('\\') => &raw[..i],
            _ => raw,
        };
        if let Some(stripped) = line.strip_suffix('\\') {
            cur.push_str(stripped);
            cur.push(' ');
        } else {
            cur.push_str(line);
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn target_to_crate(target: &str) -> Option<String> {
    if target.starts_with("test/") || target.contains("doctests") {
        return None;
    }
    let base = target.rsplit('/').next().unwrap_or(target);
    if let Some(inner) = base.strip_prefix("$(lib").and_then(|s| s.strip_suffix("_name)")) {
        return Some(inner.to_string());
    }
    if let Some(n) = base.strip_prefix("lib").and_then(|s| s.strip_suffix(".rlib")) {
        return Some(n.to_string());
    }
    if let Some(n) = base.strip_prefix("lib").and_then(|s| s.strip_suffix(".so")) {
        return Some(n.to_string());
    }
    if let Some(n) = base.strip_suffix(".o") {
        if n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Some(n.to_string());
        }
    }
    None
}

/// Expands `$(name)` references using `vars`, recursively (bounded).
fn expand(s: &str, vars: &BTreeMap<String, String>, depth: u32) -> String {
    if depth > 8 || !s.contains("$(") {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("$(") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find(')') {
            Some(j) => {
                let name = &after[..j];
                match vars.get(name) {
                    Some(v) => out.push_str(&expand(v, vars, depth + 1)),
                    None => {
                        out.push_str("$(");
                        out.push_str(name);
                        out.push(')');
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn externs(flags: &str) -> Vec<String> {
    let toks: Vec<&str> = flags.split_whitespace().collect();
    let mut out = Vec::new();
    for w in toks.windows(2) {
        if w[0] == "--extern" {
            let name = w[1].split('=').next().unwrap_or("").trim_matches('"');
            if !name.is_empty() && name != "proc_macro" {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Turns a source prerequisite into a tree path and kind.
fn source_to_path(src: &str) -> Option<(String, &'static str)> {
    if src.starts_with("$(RUST_LIB_SRC)") {
        return Some((String::new(), "toolchain"));
    }
    let rel = src.strip_prefix("$(src)/").or_else(|| src.strip_prefix("$(srctree)/$(src)/"))?;
    let tree = format!("rust/{rel}");
    let path = if tree.ends_with("/lib.rs") {
        tree[..tree.len() - "lib.rs".len()].to_string()
    } else {
        tree
    };
    let kind = if VENDORED_DIRS.iter().any(|d| path.starts_with(d)) { "vendored" } else { "first_party" };
    Some((path, kind))
}

pub fn parse(src: &str) -> Vec<CrateDef> {
    let lines = logical_lines(src);

    // Plain variable definitions (`name := value`, `name = value`).
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    for l in &lines {
        if l.starts_with(char::is_whitespace) || l.starts_with('$') {
            continue;
        }
        let (name, value) = if let Some(i) = l.find(":=") {
            (&l[..i], &l[i + 2..])
        } else if let Some(i) = l.find('=') {
            let n = &l[..i];
            if n.ends_with('+') || n.ends_with('?') || n.contains(':') {
                continue;
            }
            (n, &l[i + 1..])
        } else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() && !name.contains(' ') {
            vars.insert(name.to_string(), value.trim().to_string());
        }
    }

    let mut sources: BTreeMap<String, (String, &'static str)> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for l in &lines {
        let Some(rest) = l.strip_prefix("$(obj)/") else { continue };
        // Target ends at the first ':' that is not inside $( ).
        let mut depth = 0i32;
        let mut colon = None;
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                ':' if depth == 0 => {
                    colon = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(ci) = colon else { continue };
        let target = rest[..ci].trim();
        let Some(name) = target_to_crate(target) else { continue };
        let body = rest[ci + 1..].trim();

        if let Some(assign) = body.strip_prefix("private ") {
            let assign = assign.trim_start();
            if let Some(v) = assign.strip_prefix("rustc_target_flags") {
                let v = v.trim_start().trim_start_matches([':', '+', '?']).trim_start_matches('=');
                let expanded = expand(v, &vars, 0);
                let d = deps.entry(name).or_default();
                for e in externs(&expanded) {
                    if !d.contains(&e) {
                        d.push(e);
                    }
                }
            }
            continue;
        }

        let first = body.split_whitespace().next().unwrap_or("");
        if !first.ends_with(".rs") || first.starts_with("$(obj)") {
            continue;
        }
        if let Some(p) = source_to_path(first) {
            if !sources.contains_key(&name) {
                order.push(name.clone());
                sources.insert(name, p);
            }
        }
    }

    let known: BTreeSet<&String> = sources.keys().collect();
    let mut out: Vec<CrateDef> = order
        .iter()
        .map(|name| {
            let (path, kind) = sources[name].clone();
            let mut d: Vec<String> = deps.get(name).cloned().unwrap_or_default();
            // `--extern alloc` with no alloc rule means the toolchain's alloc; keep it.
            d.retain(|x| known.contains(x) || x == "alloc" || x == "core");
            CrateDef { name: name.clone(), kind, path, deps: d }
        })
        .collect();

    // Toolchain crates referenced but not built from the tree (e.g. alloc after v6.13).
    let referenced: BTreeSet<String> = out.iter().flat_map(|c| c.deps.iter().cloned()).collect();
    for t in ["core", "alloc"] {
        if referenced.contains(t) && !out.iter().any(|c| c.name == t) {
            out.push(CrateDef { name: t.to_string(), kind: "toolchain", path: String::new(), deps: vec![] });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/rust_Makefile.snippet");

    fn find<'a>(c: &'a [CrateDef], n: &str) -> &'a CrateDef {
        c.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("missing crate {n}: {c:?}"))
    }

    #[test]
    fn parses_fixture() {
        let c = parse(FIXTURE);
        let names: BTreeSet<&str> = c.iter().map(|x| x.name.as_str()).collect();
        let want: BTreeSet<&str> = [
            "core", "compiler_builtins", "ffi", "build_error", "bindings", "kernel", "macros",
            "proc_macro2", "quote", "syn", "pin_init", "pin_init_internal", "alloc",
        ]
        .into_iter()
        .collect();
        assert_eq!(names, want);

        let core = find(&c, "core");
        assert_eq!(core.kind, "toolchain");
        assert_eq!(core.path, "");

        let alloc = find(&c, "alloc");
        assert_eq!(alloc.kind, "vendored");
        assert_eq!(alloc.path, "rust/alloc/");

        let k = find(&c, "kernel");
        assert_eq!(k.kind, "first_party");
        assert_eq!(k.path, "rust/kernel/");
        assert_eq!(k.deps, vec!["alloc", "ffi", "pin_init", "build_error", "macros", "bindings"]);

        assert_eq!(find(&c, "ffi").path, "rust/ffi.rs");
        assert_eq!(find(&c, "pin_init").path, "rust/pin-init/src/");
        assert_eq!(find(&c, "pin_init").deps, vec!["pin_init_internal", "macros"]);
        assert_eq!(find(&c, "pin_init_internal").path, "rust/pin-init/internal/src/");

        let syn = find(&c, "syn");
        assert_eq!(syn.kind, "vendored");
        assert_eq!(syn.deps, vec!["proc_macro2", "quote"]);
        assert_eq!(find(&c, "macros").deps, vec!["proc_macro2", "quote", "syn"]);
        assert_eq!(find(&c, "macros").path, "rust/macros/");
    }

    #[test]
    fn target_names() {
        assert_eq!(target_to_crate("kernel.o").as_deref(), Some("kernel"));
        assert_eq!(target_to_crate("host/libsyn.rlib").as_deref(), Some("syn"));
        assert_eq!(target_to_crate("libmacros.so").as_deref(), Some("macros"));
        assert_eq!(target_to_crate("$(libmacros_name)").as_deref(), Some("macros"));
        assert_eq!(target_to_crate("exports_core_generated.h"), None);
        assert_eq!(target_to_crate("test/libzerocopy.rlib"), None);
    }
}
