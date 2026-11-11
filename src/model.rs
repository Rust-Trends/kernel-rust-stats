//! Output types. Field names and order follow schema.md exactly.

use serde::{Deserialize, Serialize};

use crate::areas::Area;
use crate::visit::{Items, Unsafe};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loc {
    pub files: u64,
    pub code: u64,
    pub comments: u64,
    pub blanks: u64,
}

impl Loc {
    pub fn add(&mut self, o: &Loc) {
        self.files += o.files;
        self.code += o.code;
        self.comments += o.comments;
        self.blanks += o.blanks;
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ByArea<T> {
    pub core: T,
    pub drivers: T,
    pub samples: T,
    pub other: T,
    pub vendored: T,
}

impl<T> ByArea<T> {
    pub fn get_mut(&mut self, a: Area) -> &mut T {
        match a {
            Area::Core => &mut self.core,
            Area::Drivers => &mut self.drivers,
            Area::Samples => &mut self.samples,
            Area::Other => &mut self.other,
            Area::Vendored => &mut self.vendored,
        }
    }

    pub fn get(&self, a: Area) -> &T {
        match a {
            Area::Core => &self.core,
            Area::Drivers => &self.drivers,
            Area::Samples => &self.samples,
            Area::Other => &self.other,
            Area::Vendored => &self.vendored,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocBlock {
    pub rust: Loc,
    pub rust_by_area: ByArea<Loc>,
    pub c: Loc,
    pub h: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameCalls {
    pub name: String,
    pub calls: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ffi {
    pub call_sites: u64,
    pub distinct_functions: u64,
    pub top: Vec<NameCalls>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleFiles {
    pub module: String,
    pub files: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Imports {
    pub kernel_top: Vec<ModuleFiles>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub id: String,
    pub label: String,
    pub path: String,
    pub area: String,
    pub files: u64,
    pub code: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crate {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub code: u64,
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    pub version: String,
    pub is_rc: bool,
    pub date: String,
    pub loc: LocBlock,
    pub rust_share: f64,
    pub rust_share_first_party: f64,
    pub items: Items,
    pub items_by_area: ByArea<Items>,
    #[serde(rename = "unsafe")]
    pub unsafe_: Unsafe,
    pub unsafe_by_area: ByArea<Unsafe>,
    pub ffi: Ffi,
    pub imports: Imports,
    pub components: Vec<Component>,
    pub crates: Vec<Crate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaDef {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analyzer {
    pub name: String,
    pub version: String,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentSpan {
    pub id: String,
    pub label: String,
    pub path: String,
    pub area: String,
    pub first_tag: String,
    pub last_tag: String,
    pub latest_code: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateSpan {
    pub name: String,
    pub kind: String,
    pub first_tag: String,
    pub last_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossCheck {
    pub tag: String,
    pub source: String,
    pub url: String,
    pub rust_code: u64,
    pub c_code: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub schema_version: u32,
    pub generated_at: String,
    pub source: String,
    pub analyzer: Analyzer,
    pub areas: Vec<AreaDef>,
    pub releases: Vec<Release>,
    pub component_timeline: Vec<ComponentSpan>,
    pub crate_timeline: Vec<CrateSpan>,
    pub cross_checks: Vec<CrossCheck>,
}
