//! syn-based walk over one Rust source file.
//!
//! Macro invocation bodies (`module! { ... }`, `macro_rules!` bodies, `quote! { ... }`) are token
//! streams to syn and are not visited.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Items {
    pub fn_free: u64,
    pub fn_impl: u64,
    pub fn_trait: u64,
    #[serde(rename = "struct")]
    pub struct_: u64,
    #[serde(rename = "enum")]
    pub enum_: u64,
    pub union: u64,
    #[serde(rename = "trait")]
    pub trait_: u64,
    pub impl_inherent: u64,
    pub impl_trait: u64,
    pub macro_rules: u64,
    pub proc_macro: u64,
    #[serde(rename = "const")]
    pub const_: u64,
    #[serde(rename = "static")]
    pub static_: u64,
    #[serde(rename = "mod")]
    pub mod_: u64,
    pub type_alias: u64,
    #[serde(rename = "use")]
    pub use_: u64,
    pub parse_errors: u64,
}

impl Items {
    pub fn add(&mut self, o: &Items) {
        self.fn_free += o.fn_free;
        self.fn_impl += o.fn_impl;
        self.fn_trait += o.fn_trait;
        self.struct_ += o.struct_;
        self.enum_ += o.enum_;
        self.union += o.union;
        self.trait_ += o.trait_;
        self.impl_inherent += o.impl_inherent;
        self.impl_trait += o.impl_trait;
        self.macro_rules += o.macro_rules;
        self.proc_macro += o.proc_macro;
        self.const_ += o.const_;
        self.static_ += o.static_;
        self.mod_ += o.mod_;
        self.type_alias += o.type_alias;
        self.use_ += o.use_;
        self.parse_errors += o.parse_errors;
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unsafe {
    pub blocks: u64,
    pub blocks_with_safety_comment: u64,
    pub fns: u64,
    pub impls: u64,
    pub traits: u64,
}

impl Unsafe {
    pub fn add(&mut self, o: &Unsafe) {
        self.blocks += o.blocks;
        self.blocks_with_safety_comment += o.blocks_with_safety_comment;
        self.fns += o.fns;
        self.impls += o.impls;
        self.traits += o.traits;
    }
}

/// Everything collected from one file.
#[derive(Debug, Default, Clone)]
pub struct FileFacts {
    pub items: Items,
    pub unsafe_: Unsafe,
    /// `bindings::<fn>` -> number of call sites.
    pub ffi_calls: BTreeMap<String, u64>,
    /// First segments of `use kernel::...` in this file (deduplicated).
    pub kernel_imports: BTreeSet<String>,
}

/// Parses `src` and collects facts. A parse failure yields `parse_errors == 1` and nothing else.
pub fn analyze_source(src: &str) -> FileFacts {
    let mut facts = FileFacts::default();
    match syn::parse_file(src) {
        Ok(file) => {
            let lines: Vec<&str> = src.lines().collect();
            let mut v = Visitor { facts: &mut facts, lines: &lines };
            v.visit_file(&file);
        }
        Err(_) => facts.items.parse_errors = 1,
    }
    // Span locations are kept in a thread-local source map that would otherwise grow forever.
    proc_macro2::extra::invalidate_current_thread_spans();
    facts
}

struct Visitor<'a> {
    facts: &'a mut FileFacts,
    lines: &'a [&'a str],
}

/// `// SAFETY:` on the block's own line or one of the 3 lines above it (1-based `line`).
pub fn has_safety_comment(lines: &[&str], line: usize) -> bool {
    if line == 0 {
        return false;
    }
    let end = line.min(lines.len());
    let start = line.saturating_sub(4);
    lines[start..end].iter().any(|l| l.contains("// SAFETY:"))
}

fn is_proc_macro_fn(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("proc_macro")
            || a.path().is_ident("proc_macro_derive")
            || a.path().is_ident("proc_macro_attribute")
    })
}

fn collect_use_tree(tree: &syn::UseTree, out: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Path(p) => {
            let seg = p.ident.to_string();
            if seg != "self" && seg != "super" && seg != "crate" {
                out.insert(seg);
            }
        }
        syn::UseTree::Name(n) => {
            let s = n.ident.to_string();
            if s != "self" {
                out.insert(s);
            }
        }
        syn::UseTree::Rename(r) => {
            if r.ident != "self" {
                out.insert(r.ident.to_string());
            }
        }
        syn::UseTree::Group(g) => {
            for t in &g.items {
                collect_use_tree(t, out);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

/// Collects `kernel::<first>` segments from a `use` tree (handles `use {kernel::a, ...}` too).
fn kernel_imports(tree: &syn::UseTree, out: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Path(p) if p.ident == "kernel" => collect_use_tree(&p.tree, out),
        syn::UseTree::Group(g) => {
            for t in &g.items {
                kernel_imports(t, out);
            }
        }
        _ => {}
    }
}

impl<'ast> Visit<'ast> for Visitor<'_> {
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        self.facts.items.fn_free += 1;
        if is_proc_macro_fn(&i.attrs) {
            self.facts.items.proc_macro += 1;
        }
        if i.sig.unsafety.is_some() {
            self.facts.unsafe_.fns += 1;
        }
        visit::visit_item_fn(self, i);
    }

    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        self.facts.items.fn_impl += 1;
        if i.sig.unsafety.is_some() {
            self.facts.unsafe_.fns += 1;
        }
        visit::visit_impl_item_fn(self, i);
    }

    fn visit_trait_item_fn(&mut self, i: &'ast syn::TraitItemFn) {
        self.facts.items.fn_trait += 1;
        if i.sig.unsafety.is_some() {
            self.facts.unsafe_.fns += 1;
        }
        visit::visit_trait_item_fn(self, i);
    }

    fn visit_item_struct(&mut self, i: &'ast syn::ItemStruct) {
        self.facts.items.struct_ += 1;
        visit::visit_item_struct(self, i);
    }

    fn visit_item_enum(&mut self, i: &'ast syn::ItemEnum) {
        self.facts.items.enum_ += 1;
        visit::visit_item_enum(self, i);
    }

    fn visit_item_union(&mut self, i: &'ast syn::ItemUnion) {
        self.facts.items.union += 1;
        visit::visit_item_union(self, i);
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        self.facts.items.trait_ += 1;
        if i.unsafety.is_some() {
            self.facts.unsafe_.traits += 1;
        }
        visit::visit_item_trait(self, i);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if i.trait_.is_some() {
            self.facts.items.impl_trait += 1;
        } else {
            self.facts.items.impl_inherent += 1;
        }
        if i.unsafety.is_some() {
            self.facts.unsafe_.impls += 1;
        }
        visit::visit_item_impl(self, i);
    }

    fn visit_item_macro(&mut self, i: &'ast syn::ItemMacro) {
        if i.mac.path.is_ident("macro_rules") {
            self.facts.items.macro_rules += 1;
        }
        visit::visit_item_macro(self, i);
    }

    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        self.facts.items.const_ += 1;
        visit::visit_item_const(self, i);
    }

    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        self.facts.items.static_ += 1;
        visit::visit_item_static(self, i);
    }

    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.facts.items.mod_ += 1;
        visit::visit_item_mod(self, i);
    }

    fn visit_item_type(&mut self, i: &'ast syn::ItemType) {
        self.facts.items.type_alias += 1;
        visit::visit_item_type(self, i);
    }

    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        self.facts.items.use_ += 1;
        // `use ::kernel::...` has a leading colon but the same tree.
        kernel_imports(&i.tree, &mut self.facts.kernel_imports);
        visit::visit_item_use(self, i);
    }

    fn visit_expr_unsafe(&mut self, e: &'ast syn::ExprUnsafe) {
        self.facts.unsafe_.blocks += 1;
        let line = e.unsafe_token.span().start().line;
        if has_safety_comment(self.lines, line) {
            self.facts.unsafe_.blocks_with_safety_comment += 1;
        }
        visit::visit_expr_unsafe(self, e);
    }

    fn visit_expr_call(&mut self, e: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = &*e.func {
            let segs = &p.path.segments;
            // The callee must be an item directly in `bindings` (`bindings::foo`,
            // `kernel::bindings::foo`); `bindings::Foo::default()` is a Rust trait call.
            if segs.len() >= 2 && segs[segs.len() - 2].ident == "bindings" {
                let name = format!("bindings::{}", segs.last().unwrap().ident);
                *self.facts.ffi_calls.entry(name).or_default() += 1;
            }
        }
        visit::visit_expr_call(self, e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/sample.rs");

    #[test]
    fn items() {
        let f = analyze_source(FIXTURE);
        let i = f.items;
        assert_eq!(i.parse_errors, 0);
        assert_eq!(i.fn_free, 4, "free fns: helper, inner_fn (in mod), unsafe_free, derive_it");
        assert_eq!(i.fn_impl, 3);
        assert_eq!(i.fn_trait, 2);
        assert_eq!(i.struct_, 1);
        assert_eq!(i.enum_, 1);
        assert_eq!(i.union, 1);
        assert_eq!(i.trait_, 2);
        assert_eq!(i.impl_inherent, 1);
        assert_eq!(i.impl_trait, 2);
        assert_eq!(i.macro_rules, 1);
        assert_eq!(i.proc_macro, 1);
        assert_eq!(i.const_, 1);
        assert_eq!(i.static_, 1);
        assert_eq!(i.mod_, 1);
        assert_eq!(i.type_alias, 1);
        assert_eq!(i.use_, 4);
    }

    #[test]
    fn unsafe_and_safety() {
        let f = analyze_source(FIXTURE);
        let u = f.unsafe_;
        assert_eq!(u.blocks, 4);
        assert_eq!(u.blocks_with_safety_comment, 2);
        assert_eq!(u.fns, 3, "unsafe free fn, unsafe trait fn decl, unsafe impl fn");
        assert_eq!(u.impls, 1);
        assert_eq!(u.traits, 1);
    }

    #[test]
    fn ffi_calls() {
        let f = analyze_source(FIXTURE);
        assert_eq!(f.ffi_calls.get("bindings::mutex_lock"), Some(&2));
        assert_eq!(f.ffi_calls.get("bindings::kfree"), Some(&1));
        assert_eq!(f.ffi_calls.len(), 2, "method calls and plain paths are not FFI: {:?}", f.ffi_calls);
    }

    #[test]
    fn kernel_imports() {
        let f = analyze_source(FIXTURE);
        let got: Vec<&str> = f.kernel_imports.iter().map(|s| s.as_str()).collect();
        assert_eq!(got, vec!["c_str", "device", "prelude", "sync"]);
    }

    #[test]
    fn parse_error() {
        let f = analyze_source("fn broken( {");
        assert_eq!(f.items.parse_errors, 1);
        assert_eq!(f.items.fn_free, 0);
    }

    #[test]
    fn safety_window() {
        let lines = ["// SAFETY: ok", "a", "b", "c", "unsafe {}"];
        assert!(!has_safety_comment(&lines, 5), "4 lines above is outside the window");
        assert!(has_safety_comment(&lines, 4));
        assert!(has_safety_comment(&lines, 1));
    }
}
