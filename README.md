# kernel-rust-stats

Measures Rust in the Linux kernel, release by release, and writes `linux-rust.json`, the data
behind the [Rust in the Linux kernel](https://rust-trends.com/linux-rust/) page on rust-trends.com.

For every final kernel release from v6.1 onward, plus the newest `-rc` when it is newer than the
newest final, it reports:

- **Lines of code**: Rust (`.rs`), C (`.c`) and C headers (`.h`), as code, comment and blank lines.
  Rust is split into five areas: core abstractions (`rust/`), drivers, samples, other, and
  vendored third-party crates.
- **Rust share**: Rust code lines / (Rust + C + header code lines), with and without vendored crates.
- **Items**: functions (free, impl, trait), structs, enums, unions, traits, impls, `macro_rules!`,
  proc macros, consts, statics, modules, type aliases and `use` items.
- **unsafe**: `unsafe {}` blocks, how many have a `// SAFETY:` comment, unsafe fns, impls and traits.
- **FFI call sites**: calls from Rust into C through the `bindings` crate, with the most called functions.
- **kernel:: imports**: which `kernel::` modules code outside `rust/` uses.
- **Components**: named Rust drivers and subsystems (nova-core, Binder, Tyr, rnull, ...).
- **Crates**: the crates `rust/Makefile` builds, their size and dependencies.

The output format is specified in [`schema.md`](schema.md). That file is the contract; consumers
read only what it documents.

## Running

Requirements: a Rust toolchain, `git` and `curl` on `PATH`, and network access to
`cdn.kernel.org`, `git.kernel.org` and `github.com`.

```sh
cargo build --release

# Analyze every release that is not cached yet, then write linux-rust.json.
./target/release/kernel-rust-stats run

# Only some tags, or re-analyze cached ones.
./target/release/kernel-rust-stats run --tags v6.18,v6.19 --refresh

# Rebuild linux-rust.json from cache/ without any network access.
./target/release/kernel-rust-stats assemble --cache-dir cache --out linux-rust.json
```

Options:

| Option | Default | Meaning |
|---|---|---|
| `--tags a,b` | discovered | Tags to analyze instead of discovering them with `git ls-remote` |
| `--out FILE` | `linux-rust.json` | Output file |
| `--cache-dir DIR` | `cache` | Per-tag results, `DIR/<tag>.json` |
| `--jobs N` | `3` | Tags analyzed in parallel (threads) |
| `--refresh` | off | Re-analyze tags that are already cached |
| `--cross-checks FILE` | `cross_checks.json` | External reference numbers copied into the output |

`run` discovers tags with `git ls-remote --tags https://github.com/torvalds/linux`, keeping
`vX.Y` and `vX.Y-rcN` for majors 6 to 8 (peeled `^{}` refs are ignored). It exits non-zero if any
tag fails. Progress goes to stderr.

`assemble` reads every `cache/*.json`, applies the same selection rule (finals plus the newest rc
if newer than the newest final), so a cached older rc drops out once its final is cached, and
derives the timelines.

The per-tag results in `cache/` are committed, a few KB each, so the published numbers can be
reproduced and checked without downloading anything.

### Nothing touches the disk

Each tarball (about 150 MB compressed, 1.5 GB unpacked) is streamed: `curl` writes to a pipe,
the pipe goes through the xz or gzip decoder into a `tar` reader, and each `.rs`, `.c` and `.h`
entry is analyzed in memory and dropped. Only the small per-tag JSON is written.

Tarballs: finals from `https://cdn.kernel.org/pub/linux/kernel/vX.x/linux-X.Y.tar.xz`, rc tags
from `https://git.kernel.org/torvalds/t/linux-X.Y-rcN.tar.gz` (falling back to GitHub's codeload
for the tag). The release date is the mtime of the tarball entries, which `git archive` sets to the
commit time of the tagged commit.

## Method

### Line counts

Lines are counted by a small built-in counter (`src/loc.rs`) that follows the rules of
[tokei](https://github.com/XAMPPRocky/tokei): a line is blank if empty, a comment if it holds only
comments (or starts with `//`, or began inside a block comment), and code otherwise. Strings,
raw strings, char literals and nested Rust block comments are tracked. Rust doc comments, including
the examples inside them, count as comments.

We started with the `tokei` crate itself, but its dependency tree (file walking, parallelism,
config parsing) is large for what this tool needs. The built-in counter was checked against tokei
15 over every `.rs`, `.c` and `.h` file of v6.1 and v6.19:

| v6.19 | this tool | tokei 15 | difference |
|---|---:|---:|---:|
| Rust code | 89,454 | 89,411 | +0.05% |
| C (`.c`) code | 19,196,331 | 19,201,517 | -0.03% |
| Header (`.h`) code | 8,035,492 | 8,035,538 | -0.001% |

The remaining differences are mostly char literals such as `'"'`, which tokei reads as the start
of a string. tokei puts Rust doc comments in a separate Markdown child entry; its Rust *code*
figure is the one compared here (with child languages folded in, tokei reports 90,428 because it
counts code inside doc examples).

### Areas

`src/areas.rs` holds all path tables:

| Area | Paths |
|---|---|
| vendored | `rust/{alloc,syn,quote,proc-macro2,zerocopy,zerocopy-derive}/` |
| core | the rest of `rust/` |
| drivers | `drivers/` |
| samples | `samples/` |
| other | everything else (`scripts/`, `lib/`, `mm/`, `tools/`, ...) |

`rust/alloc/` was a trimmed copy of the standard library's `alloc` crate (v6.1 to v6.9; v6.10 to
v6.12 used the toolchain's `alloc`, and from v6.13 the kernel has its own allocation types).
`syn`, `quote` and `proc-macro2` were vendored for the proc-macro crates in v6.19, and
`zerocopy`/`zerocopy-derive` in v7.2. Vendored code is about half of all Rust from v6.19 on, which
is why the output has a first-party share next to the overall share.

### syn walk

Every `.rs` file is parsed with `syn` (full syntax) and visited:

- **Items** are counted where syn sees them. `fn_free` counts every `fn` item, including the
  rare `fn` nested in a function body. `const` and `static` count items, not associated consts.
- **unsafe**: `blocks` counts `unsafe { }` expressions, `fns` counts `unsafe fn` in free, impl and
  trait position, and `impls` and `traits` count `unsafe impl` and `unsafe trait`.
- **SAFETY comments**: a block counts as documented when `// SAFETY:` appears on the line of the
  `unsafe` keyword or in the 3 lines above it. A SAFETY comment longer than 3 lines above the
  block is missed, so this is a lower bound.
- **FFI call sites**: call expressions whose callee path has `bindings` as its second-to-last
  segment (`bindings::foo(..)`, `kernel::bindings::foo(..)`, `crate::bindings::foo(..)`), in
  first-party code only. Calls through function pointers or wrappers, and calls inside macro
  bodies, are not seen.
- **Imports**: for files outside `rust/`, the first segment after `kernel::` in every `use`, counted
  once per file (`use kernel::{a, b::c}` counts `a` and `b`).
- Macro invocation bodies (`module! { .. }`, `macro_rules!` arms, `quote! { .. }`) are opaque
  token streams to syn and are not visited. A fair amount of `unsafe` in `rust/kernel` lives in
  `macro_rules!` bodies and is not counted.
- Files that fail to parse are skipped and counted in `parse_errors`. In practice these are only
  files of the vendored `rust/alloc` in v6.1 to v6.5 (2 to 4 files per release), which use unstable
  syntax that `syn` does not accept; from v6.6 every file parses.

### Components

A curated table in `src/areas.rs` names known Rust components and their paths, including renames
(`drivers/block/rnull.rs` became `drivers/block/rnull/`; `drm_panic_qr.rs` became
`drm_panic_helper_qr.rs`). Any other `.rs` file under `drivers/` becomes a component of its own
(a directory with several Rust files and no C files becomes one component), and the run prints
`unmatched component: ...` so the table can be extended.

### Crates (heuristic)

The crate graph comes from `rust/Makefile`, which is not evaluated. `src/makefile.rs`:

1. joins continuation lines and records plain variable definitions;
2. treats every rule `$(obj)/<target>: <source.rs> ...` as a crate, mapping `X.o`, `libX.rlib`,
   `host/libX.rlib`, `libX.so` and `$(libX_name)` to the crate name `X`;
3. reads dependencies from the target's `rustc_target_flags` (`--extern X`), expanding `$(var)`
   references, and keeps only names that are crates themselves (plus toolchain `core` and `alloc`);
4. sizes each crate by summing the code lines under its source directory (`rust/kernel/`,
   `rust/pin-init/src/`, ...) or of its single file (`rust/ffi.rs`).

Known gaps: crates are only listed if they have such a rule; implicit dependencies (every crate on
`core`, proc macros on `proc_macro`) are not listed; and a crate's size is its source tree only,
not code it pulls in with `include!`. `core` (and `alloc` when it comes from the toolchain) are
listed with kind `toolchain` and code 0. The graph was spot-checked by hand for v6.1, v6.19 and
the newest tag.

## Cross-check with LWN

LWN's [development statistics for 6.19](https://lwn.net/Articles/1057807/) (editor comment) give
code-only line counts. `cross_checks.json` holds them and they are copied into the output.

| Tag | | LWN | this tool | difference |
|---|---|---:|---:|---:|
| v6.17 | Rust code | 23,163 | 23,266 | +0.4% |
| v6.17 | C code | 18,951,316 | 18,924,721 (`.c`) | -0.1% |
| v6.18 | Rust code | 36,077 | 36,180 | +0.3% |
| v6.18 | C code | 19,041,645 | 19,015,046 (`.c`) | -0.1% |
| v6.19 | Rust code | 89,286 | 89,454 | +0.2% |
| v6.19 | C code | 19,222,910 | 19,196,331 (`.c`) | -0.1% |

LWN's "C" figure matches `.c` files only: adding headers would give 27.2 million lines for v6.19,
about 40% more than LWN reports. The output keeps `.c` and `.h` separate, and `rust_share` uses
`.c` + `.h` as the schema defines, so it is lower than a share computed against LWN's C figure.
The jump from v6.18 to v6.19 is mostly the vendored `syn`, `quote` and `proc-macro2` (47,021
lines); LWN's numbers include them too.

## Limitations

- Line counts treat every `.c` and `.h` file in the tarball alike, including `tools/`, `scripts/`
  and generated tables that are checked in; they say nothing about what a given build compiles.
- The tool sees source text, not a configured build: no `cfg` evaluation, no generated bindings.
- Component and crate detection are heuristics backed by hand-maintained tables.
- The SAFETY measure is textual (see above).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
