//! Path tables: which area a file belongs to, which directories are vendored, and the curated
//! list of named Rust components. All path knowledge lives in this module.

/// The fixed set and order of areas (see schema.md).
pub const AREAS: [(&str, &str); 5] = [
    ("core", "Core abstractions (rust/)"),
    ("drivers", "Drivers"),
    ("samples", "Samples"),
    ("other", "Other (lib, mm, scripts, tools)"),
    ("vendored", "Vendored third-party crates"),
];

/// Third-party crates copied into `rust/`.
pub const VENDORED_DIRS: &[&str] = &[
    "rust/alloc/",
    "rust/syn/",
    "rust/quote/",
    "rust/proc-macro2/",
    "rust/zerocopy/",
    "rust/zerocopy-derive/",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Area {
    Core = 0,
    Drivers = 1,
    Samples = 2,
    Other = 3,
    Vendored = 4,
}

impl Area {
    pub const ALL: [Area; 5] = [Area::Core, Area::Drivers, Area::Samples, Area::Other, Area::Vendored];

    pub fn id(self) -> &'static str {
        AREAS[self as usize].0
    }

    pub fn is_first_party(self) -> bool {
        self != Area::Vendored
    }
}

/// Classifies a path relative to the kernel tree root (e.g. `drivers/net/phy/qt2025.rs`).
pub fn classify(path: &str) -> Area {
    if VENDORED_DIRS.iter().any(|d| path.starts_with(d)) {
        Area::Vendored
    } else if path.starts_with("rust/") {
        Area::Core
    } else if path.starts_with("drivers/") {
        Area::Drivers
    } else if path.starts_with("samples/") {
        Area::Samples
    } else {
        Area::Other
    }
}

/// A named unit of Rust code outside `rust/`.
///
/// Each entry in `paths` is either a directory prefix (ends with `/`) or an exact file path.
/// Several paths cover renames and moves; the release reports the path(s) it actually matched.
pub struct ComponentDef {
    pub id: &'static str,
    pub label: &'static str,
    pub paths: &'static [&'static str],
}

pub const COMPONENTS: &[ComponentDef] = &[
    ComponentDef {
        id: "ax88796b",
        label: "ASIX AX88796B PHY driver",
        paths: &["drivers/net/phy/ax88796b_rust.rs"],
    },
    ComponentDef {
        id: "qt2025",
        label: "Marvell QT2025 PHY driver",
        paths: &["drivers/net/phy/qt2025.rs"],
    },
    ComponentDef {
        id: "rnull",
        label: "rnull null block driver",
        paths: &["drivers/block/rnull.rs", "drivers/block/rnull/"],
    },
    ComponentDef {
        id: "drm-panic-qr",
        label: "DRM panic QR code generator",
        paths: &["drivers/gpu/drm/drm_panic_qr.rs", "drivers/gpu/drm/drm_panic_helper_qr.rs"],
    },
    ComponentDef {
        id: "nova-core",
        label: "NVIDIA nova-core (GSP)",
        paths: &["drivers/gpu/nova-core/"],
    },
    ComponentDef {
        id: "nova-drm",
        label: "NVIDIA nova DRM driver",
        paths: &["drivers/gpu/drm/nova/"],
    },
    ComponentDef {
        id: "tyr",
        label: "Tyr (Arm Mali GPU DRM driver)",
        paths: &["drivers/gpu/drm/tyr/"],
    },
    ComponentDef {
        id: "binder",
        label: "Android Binder IPC driver",
        paths: &["drivers/android/binder/"],
    },
    ComponentDef {
        id: "rcpufreq-dt",
        label: "Device-tree cpufreq driver (Rust)",
        paths: &["drivers/cpufreq/rcpufreq_dt.rs"],
    },
    ComponentDef {
        id: "pwm-th1520",
        label: "T-HEAD TH1520 PWM driver",
        paths: &["drivers/pwm/pwm_th1520.rs"],
    },
    ComponentDef {
        id: "samples",
        label: "Rust samples",
        paths: &["samples/rust/"],
    },
    ComponentDef {
        id: "kasan-test",
        label: "KASAN Rust test",
        paths: &["mm/kasan/kasan_test_rust.rs"],
    },
    ComponentDef {
        id: "find-bit-benchmark",
        label: "find_bit benchmark (Rust)",
        paths: &["lib/find_bit_benchmark_rust.rs"],
    },
];

/// Returns the curated component and the matched table path for a file, if any.
pub fn match_component(path: &str) -> Option<(&'static ComponentDef, &'static str)> {
    for def in COMPONENTS {
        for p in def.paths {
            let hit = if p.ends_with('/') { path.starts_with(p) } else { path == *p };
            if hit {
                return Some((def, p));
            }
        }
    }
    None
}

/// Directory part of a path, with trailing slash (`a/b/c.rs` -> `a/b/`).
pub fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "",
    }
}

/// File stem (`a/b/c_rust.rs` -> `c_rust`).
pub fn file_stem(path: &str) -> &str {
    let name = &path[path.rfind('/').map_or(0, |i| i + 1)..];
    name.strip_suffix(".rs").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_areas() {
        assert_eq!(classify("rust/kernel/lib.rs"), Area::Core);
        assert_eq!(classify("rust/macros/lib.rs"), Area::Core);
        assert_eq!(classify("rust/pin-init/src/lib.rs"), Area::Core);
        assert_eq!(classify("rust/alloc/vec/mod.rs"), Area::Vendored);
        assert_eq!(classify("rust/syn/lib.rs"), Area::Vendored);
        assert_eq!(classify("rust/proc-macro2/lib.rs"), Area::Vendored);
        assert_eq!(classify("rust/zerocopy/src/lib.rs"), Area::Vendored);
        assert_eq!(classify("rust/zerocopy-derive/lib.rs"), Area::Vendored);
        assert_eq!(classify("rust/zerocopyx/lib.rs"), Area::Core);
        assert_eq!(classify("drivers/gpu/nova-core/gpu.rs"), Area::Drivers);
        assert_eq!(classify("samples/rust/rust_minimal.rs"), Area::Samples);
        assert_eq!(classify("scripts/generate_rust_target.rs"), Area::Other);
        assert_eq!(classify("mm/kasan/kasan_test_rust.rs"), Area::Other);
        assert_eq!(classify("rustfoo/x.rs"), Area::Other);
    }

    #[test]
    fn components_match() {
        assert_eq!(match_component("drivers/block/rnull.rs").unwrap().0.id, "rnull");
        assert_eq!(match_component("drivers/block/rnull/configfs.rs").unwrap().0.id, "rnull");
        assert_eq!(match_component("drivers/gpu/drm/drm_panic_qr.rs").unwrap().0.id, "drm-panic-qr");
        assert_eq!(match_component("drivers/gpu/drm/drm_panic_helper_qr.rs").unwrap().0.id, "drm-panic-qr");
        assert_eq!(match_component("drivers/gpu/drm/nova/driver.rs").unwrap().0.id, "nova-drm");
        assert_eq!(match_component("drivers/gpu/nova-core/gsp/fw.rs").unwrap().0.id, "nova-core");
        assert!(match_component("drivers/gpu/drm/novax.rs").is_none());
        assert_eq!(file_stem("drivers/a/b_rust.rs"), "b_rust");
        assert_eq!(parent_dir("drivers/a/b.rs"), "drivers/a/");
    }
}
