# `linux-rust.json` — schema v1

The single contract between the analyzer (`kernel-rust-stats`) and anything that renders it
(the rust-trends.com `/linux-rust/` page). Consumers read only what is documented here; adding
fields is fine, renaming or removing them bumps `schema_version`.

All line counts are produced by the `tokei` crate (code = non-blank, non-comment lines).
All lists are already sorted (releases oldest → newest, "top" lists by count descending).

```jsonc
{
  "schema_version": 1,
  "generated_at": "2026-11-11T12:00:00Z",          // RFC 3339, UTC
  "source": "https://github.com/torvalds/linux",
  "analyzer": {
    "name": "kernel-rust-stats",
    "version": "0.1.0",
    "repo": "https://github.com/Rust-Trends/kernel-rust-stats"
  },

  // Fixed set and order of areas. Every *_by_area object uses exactly these keys.
  "areas": [
    { "id": "core",     "label": "Core abstractions (rust/)" },   // rust/** minus vendored
    { "id": "drivers",  "label": "Drivers" },                     // drivers/**
    { "id": "samples",  "label": "Samples" },                     // samples/**
    { "id": "other",    "label": "Other (lib, mm, scripts, tools)" },
    { "id": "vendored", "label": "Vendored third-party crates" }  // rust/{alloc,syn,quote,proc-macro2,zerocopy,zerocopy-derive}/**
  ],

  // Every final release v6.1 … newest, plus the newest -rc tag if it is newer than the newest final.
  "releases": [
    {
      "tag": "v6.19",
      "version": "6.19",
      "is_rc": false,
      "date": "2026-02-08",                 // commit date of the tag (from the tarball entry mtime), YYYY-MM-DD

      "loc": {
        "rust": { "files": 0, "code": 0, "comments": 0, "blanks": 0 },   // all .rs files, all areas
        "rust_by_area": {
          "core":     { "files": 0, "code": 0, "comments": 0, "blanks": 0 },
          "drivers":  { "files": 0, "code": 0, "comments": 0, "blanks": 0 },
          "samples":  { "files": 0, "code": 0, "comments": 0, "blanks": 0 },
          "other":    { "files": 0, "code": 0, "comments": 0, "blanks": 0 },
          "vendored": { "files": 0, "code": 0, "comments": 0, "blanks": 0 }
        },
        "c": { "files": 0, "code": 0, "comments": 0, "blanks": 0 },      // *.c
        "h": { "files": 0, "code": 0, "comments": 0, "blanks": 0 }       // *.h
      },

      // code-line shares, 0..1:  rust.code / (rust.code + c.code + h.code)
      "rust_share": 0.0,
      // same but Rust minus the vendored area, in numerator and denominator
      "rust_share_first_party": 0.0,

      // syn-based counts. Files that fail to parse are skipped and counted in parse_errors.
      // Contents of macro invocations (module! {...}, macro_rules! bodies) are not visited.
      "items": {
        "fn_free": 0,        // fn at module level (incl. inside inline `mod {}`)
        "fn_impl": 0,        // fn inside impl blocks (methods / associated fns)
        "fn_trait": 0,       // fn declared inside trait definitions
        "struct": 0, "enum": 0, "union": 0, "trait": 0,
        "impl_inherent": 0,  // impl Foo {}
        "impl_trait": 0,     // impl Trait for Foo {}
        "macro_rules": 0,
        "proc_macro": 0,     // fns with #[proc_macro], #[proc_macro_derive] or #[proc_macro_attribute]
        "const": 0, "static": 0, "mod": 0, "type_alias": 0,
        "use": 0,            // `use` items (not individual leaves)
        "parse_errors": 0
      },
      "items_by_area": { "core": { /* same keys as items */ }, "drivers": {}, "samples": {}, "other": {}, "vendored": {} },

      "unsafe": {
        "blocks": 0,                       // unsafe { } expressions
        "blocks_with_safety_comment": 0,   // a `// SAFETY:` comment in the 3 source lines above the block or on its line
        "fns": 0,                          // unsafe fn (free, impl and trait)
        "impls": 0,                        // unsafe impl
        "traits": 0                        // unsafe trait
      },
      "unsafe_by_area": { "core": { /* same keys as unsafe */ }, "drivers": {}, "samples": {}, "other": {}, "vendored": {} },

      // Calls from Rust into C: call expressions whose callee path contains a `bindings` segment
      // (bindings::foo(..), kernel::bindings::foo(..), crate::bindings::foo(..)). First-party areas only.
      "ffi": {
        "call_sites": 0,
        "distinct_functions": 0,
        "top": [ { "name": "bindings::mutex_lock", "calls": 0 } ]   // top 25, name normalised to "bindings::<fn>"
      },

      // Which kernel:: modules Rust outside rust/ uses (drivers + samples + other).
      // Count = number of files with at least one `use kernel::<module>…` (first path segment after kernel::;
      // `use kernel::{a, b::c}` counts a and b; `kernel::prelude::*` counts prelude).
      "imports": {
        "kernel_top": [ { "module": "prelude", "files": 0 } ]       // top 25
      },

      // Named units of Rust code (drivers and other subsystems outside rust/), present in this release.
      // Detected by the analyzer from a curated path table plus a fallback for unknown drivers.
      "components": [
        { "id": "nova-core", "label": "NVIDIA nova-core (GSP)", "path": "drivers/gpu/nova-core/", "area": "drivers",
          "files": 0, "code": 0 }
      ],

      // Crates built from the tree in this release, from rust/Makefile (--extern flags) + directory sizes.
      "crates": [
        { "name": "kernel", "kind": "first_party",   // first_party | vendored | toolchain (core, alloc from sysroot: code 0)
          "path": "rust/kernel/", "code": 0, "deps": ["ffi", "macros", "bindings", "uapi", "pin_init", "build_error"] }
      ]
    }
  ],

  // Derived over all releases, for timelines. Sorted by first_tag, then id.
  "component_timeline": [
    { "id": "ax88796b", "label": "ASIX AX88796B PHY driver", "path": "drivers/net/phy/ax88796b_rust.rs",
      "area": "drivers", "first_tag": "v6.8", "last_tag": "v7.4-rc2", "latest_code": 0 }
  ],
  "crate_timeline": [
    { "name": "syn", "kind": "vendored", "first_tag": "v6.19", "last_tag": "v7.4-rc2" }
  ],

  // External reference points the page shows for credibility (hand-maintained in the analyzer repo).
  "cross_checks": [
    { "tag": "v6.19", "source": "LWN, 'Development statistics for 6.19' comments",
      "url": "https://lwn.net/Articles/1057807/", "rust_code": 89286, "c_code": 19222910 }
  ]
}
```

Notes for consumers
- The newest release is `releases[releases.length - 1]`; it may be an rc (`is_rc`).
- "Rust drivers" headline = `component_timeline` entries with `area == "drivers"` and `last_tag` == newest tag.
- The JSON is self-contained; no other files are needed to render the page.
