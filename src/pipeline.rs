//! The drules generation pipeline.
//!
//! Reads a MapCSS stylesheet, the classificator (`mapcss-mapping.csv`),
//! priorities files and dynamic tags, and produces the native drules `.bin`/
//! `.txt` files plus `types.txt`, `classificator.txt`, `visibility.txt`,
//! `colors.txt`, `patterns.txt` and re-formatted priorities files. Output is
//! byte-identical to the Python implementation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;

use crate::color::{cairo_to_hex, parse_color_hex};
use crate::compat;
use crate::condition::Condition;
use crate::drules::*;
use crate::mapcss::MapCSS;
use crate::style_chooser::{ChainMatchCache, StyleValue};

// Priority values defined in *.prio.txt files are adjusted
// to fit into the following "priorities ranges":
// [-10000; 10000): overlays (icons, captions...)
// [0; 1000)      : FG - foreground areas and lines
// [-1000; 0)     : BG-top - water, linear and areal, rendered just on top of landcover
// (-2000; -1000) : BG-by-size - landcover areas, later in core sorted by their bbox size
// The core renderer then re-adjusts those ranges as necessary to accomodate
// for special behavior and features' layer=* values.
// See drape_frontend/stylist.cpp for the details of layering logic.

// Priority range for area and line drules. Should be same as drule::kLayerPriorityRange.
pub const LAYER_PRIORITY_RANGE: i32 = 1000;
// Should be same as drule::kOverlaysMaxPriority. The overlays range is [-kOverlaysMaxPriority; kOverlaysMaxPriority),
// negative values are used for optional captions which are below most other overlays.
pub const OVERLAYS_MAX_PRIORITY: i32 = 10000;

/// The four ranges drules' priorities are arranged into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrioRangeKind {
    /// Overlays: icons, captions, path texts and shields,
    /// range [-OVERLAYS_MAX_PRIORITY; OVERLAYS_MAX_PRIORITY).
    Overlays,
    /// FG: foreground areas and lines, range [0; LAYER_PRIORITY_RANGE).
    Fg,
    /// BG-top: water, linear and areal, rendered just on top of landcover,
    /// range [-LAYER_PRIORITY_RANGE; 0).
    BgTop,
    /// BG-by-size: landcover areas, later in core sorted by their bbox size,
    /// range (-2*LAYER_PRIORITY_RANGE; -LAYER_PRIORITY_RANGE].
    BgBySize,
}

impl PrioRangeKind {
    /// All ranges in their canonical (file/dump) order.
    pub const ALL: [PrioRangeKind; 4] = [
        PrioRangeKind::Overlays,
        PrioRangeKind::Fg,
        PrioRangeKind::BgTop,
        PrioRangeKind::BgBySize,
    ];

    /// Numbering used in the `priorities_<pos>_<name>.prio.txt` file names.
    pub fn file_pos(self) -> i32 {
        match self {
            PrioRangeKind::Overlays => 4,
            PrioRangeKind::Fg => 3,
            PrioRangeKind::BgTop => 2,
            PrioRangeKind::BgBySize => 1,
        }
    }

    /// Constant added to the priorities of this range (see `get_drape_priority`).
    pub fn base(self) -> i32 {
        match self {
            PrioRangeKind::Overlays | PrioRangeKind::Fg => 0,
            PrioRangeKind::BgTop => -1000,
            PrioRangeKind::BgBySize => -2000,
        }
    }

    /// Descriptive comment written atop the dumped priorities file.
    pub fn comment(self) -> &'static str {
        match self {
            PrioRangeKind::Overlays => COMMENT_OVERLAYS,
            PrioRangeKind::Fg => COMMENT_FG,
            PrioRangeKind::BgTop => COMMENT_BG_TOP,
            PrioRangeKind::BgBySize => COMMENT_BG_BY_SIZE,
        }
    }
}

impl std::fmt::Display for PrioRangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PrioRangeKind::Overlays => "overlays",
            PrioRangeKind::Fg => "FG",
            PrioRangeKind::BgTop => "BG-top",
            PrioRangeKind::BgBySize => "BG-by-size",
        })
    }
}

const COMMENT_AUTOFORMAT: &str = "This file is automatically re-formatted and re-sorted in priorities descending order
when generate_drules.sh is run. All comments (automatic priorities of e.g. optional captions, drule types visibilities, etc.)
are generated automatically for information only. Custom formatting and comments are not preserved.
";

const COMMENT_OVERLAYS: &str = "
Overlays (icons, captions, path texts and shields) are rendered on top of all the geometry (lines, areas).
Overlays don't overlap each other, instead the ones with higher priority displace the less important ones.
Optional captions (which have an icon) are usually displayed only if there are no other overlays in their way
(technically, max overlays priority value (10000) is subtracted from their priorities automatically).
";

const COMMENT_FG: &str = "
FG geometry: foreground lines and areas (e.g. buildings) are rendered always below overlays
and always on top of background geometry (BG-top & BG-by-size) even if a foreground feature
is layer=-10 (as tunnels should be visibile over landcover and water).
";

const COMMENT_BG_TOP: &str = "
BG-top geometry: background lines and areas that should be always below foreground ones
(including e.g. layer=-10 underwater tunnels), but above background areas sorted by size (BG-by-size),
because ordering by size doesn't always work with e.g. water mapped over a forest,
so water should be on top of other landcover always, but linear waterways should be hidden beneath it.
Still, e.g. a layer=-1 BG-top feature will be rendered under a layer=0 BG-by-size feature
(so areal water tunnels are hidden beneath other landcover area) and a layer=1 landcover areas
are displayed above layer=0 BG-top.
";

const COMMENT_BG_BY_SIZE: &str = "
BG-by-size geometry: background areas rendered below BG-top and everything else.
Smaller areas are rendered above larger ones (area's size is estimated as the size of its' bounding box).
So effectively priority values of BG-by-size areas are not used at the moment.
But we might use them later for some special cases, e.g. to determine a main area type of a multi-type feature.
Keep them in a logical importance order please.
";

const COMMENT_RANGES_OVERVIEW: &str = "
Priorities ranges' rendering order overview:
- overlays (icons, captions...)
- FG: foreground areas and lines
- BG-top: water (linear and areal)
- BG-by-size: landcover areas sorted by their size
";

const COMMENT_AUTO_CAPTIONS: &str = "
    All automatic optional captions priorities are below 0.
    They follow the order of their correspoding icons.
    ";

/// A priority entry key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PrioKey {
    pub cl: String,
    pub object_id: String,
    pub auto: Option<(String, Option<String>)>,
}

impl PrioKey {
    pub fn main(cl: &str, object_id: &str) -> PrioKey {
        PrioKey {
            cl: cl.to_string(),
            object_id: object_id.to_string(),
            auto: None,
        }
    }
    pub fn auto(
        cl: &str,
        object_id: &str,
        auto_dr_type: &str,
        auto_comment: Option<String>,
    ) -> PrioKey {
        PrioKey {
            cl: cl.to_string(),
            object_id: object_id.to_string(),
            auto: Some((auto_dr_type.to_string(), auto_comment)),
        }
    }
}

#[derive(Clone, Default)]
pub struct PrioRange {
    /// Insertion-ordered like the Python dict it mirrors: the priorities file
    /// load order, with automatic priorities appended during generation. The
    /// dump's stable sort relies on this to break sort-key ties deterministically.
    pub priorities: IndexMap<PrioKey, i32>,
}

/// The four `PrioRange`s, indexable by `PrioRangeKind`.
#[derive(Clone, Default)]
pub struct PrioRanges([PrioRange; 4]);

impl PrioRanges {
    pub fn iter(&self) -> impl Iterator<Item = (PrioRangeKind, &PrioRange)> {
        PrioRangeKind::ALL
            .iter()
            .zip(self.0.iter())
            .map(|(k, r)| (*k, r))
    }
}

impl std::ops::Index<PrioRangeKind> for PrioRanges {
    type Output = PrioRange;

    fn index(&self, kind: PrioRangeKind) -> &PrioRange {
        &self.0[kind as usize]
    }
}

impl std::ops::IndexMut<PrioRangeKind> for PrioRanges {
    fn index_mut(&mut self, kind: PrioRangeKind) -> &mut PrioRange {
        &mut self.0[kind as usize]
    }
}

/// A visibility entry: (dr_type, auto_comment) -> {object_id: {zooms}}.
#[derive(Clone)]
pub struct VisEntry {
    pub dr_type: String,
    pub auto_comment: Option<String>,
    pub object_ids: BTreeMap<String, BTreeSet<i32>>,
}

pub type Visibilities = HashMap<String, Vec<VisEntry>>;

/// CLI options, mirroring the Python `OptionParser` options.
#[derive(Clone, Debug)]
pub struct Options {
    pub filename: Option<String>,
    pub minzoom: i32,
    pub maxzoom: i32,
    pub outfile: String,
    pub txt: bool,
    pub priorities_path: String,
    pub data: Option<String>,
}

/// Mutable pipeline state (the Python module-level globals).
pub struct Pipeline {
    pub prio_ranges: PrioRanges,
    pub visibilities: Visibilities,
    pub validation_errors_count: usize,
}

impl Pipeline {
    pub fn new() -> Pipeline {
        Pipeline {
            prio_ranges: PrioRanges::default(),
            visibilities: HashMap::new(),
            validation_errors_count: 0,
        }
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

pub fn to_boolean(s: &str) -> (bool, bool) {
    let s = s.to_lowercase();
    if s == "true" || s == "yes" {
        (true, true)
    } else if s == "false" || s == "no" {
        (true, false)
    } else {
        (false, false)
    }
}

fn sv_str(v: &StyleValue) -> String {
    v.py_str()
}

fn sv_float(v: &StyleValue) -> f64 {
    match v {
        StyleValue::Num(n) => *n,
        StyleValue::Str(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn sv_int(v: &StyleValue) -> i64 {
    sv_float(v) as i64
}

fn sv_kept_after_strip(v: &StyleValue) -> bool {
    !sv_str(v)
        .trim_matches(|c| c == ' ' || c == '0' || c == '.')
        .is_empty()
}

pub fn mwm_encode_color(
    colors: &mut HashSet<u32>,
    st: &HashMap<String, StyleValue>,
    prefix: &str,
    default: &str,
) -> Result<u32, String> {
    let prefix_key = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}-", prefix)
    };
    let opacity = match st.get(&format!("{}opacity", prefix_key)) {
        Some(v) => sv_float(v),
        None => 1.0,
    };
    let opacity = 255 - (255.0 * opacity) as i64;
    let default_color = StyleValue::Str(default.to_string());
    let color_val = st
        .get(&format!("{}color", prefix_key))
        .unwrap_or(&default_color);
    let color = match color_val {
        StyleValue::Str(s) => {
            parse_color_hex(s).ok_or_else(|| format!("unparseable color '{}'", s))?
        }
        StyleValue::Color(c) => cairo_to_hex(*c).to_uppercase(),
        other => {
            return Err(format!("unsupported color value: {}", sv_str(other)));
        }
    };
    let result = u32::from_str_radix(&format!("{:x}{}", opacity, &color[1..]), 16)
        .map_err(|e| format!("bad color: {}", e))?;
    colors.insert(result);
    Ok(result)
}

pub fn mwm_encode_image(st: &HashMap<String, StyleValue>, prefix: &str) -> Option<String> {
    let prefix_key = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}-", prefix)
    };
    let s = sv_str(st.get(&format!("{}image", prefix_key))?);
    Some(s[..s.len().saturating_sub(4)].to_string())
}

/// A single query_style result for one class/zoom/runtime-conditions.
pub struct QueryResult {
    pub cl: String,
    pub zoom: i32,
    pub runtime_conditions: Option<Vec<Condition>>,
    pub zstyle: Vec<HashMap<String, StyleValue>>,
}

fn rule_sort_key(st: &HashMap<String, StyleValue>) -> (i32, String) {
    let oid = st.get("object-id").map(sv_str).unwrap_or_default();
    let mut first = 0;
    if let Some(text) = st.get("text") {
        let text_str = sv_str(text);
        if !text_str.is_empty() {
            if oid != "::default" {
                first = 1;
            }
            if text_str == "none" {
                first = 2;
            }
        }
    }
    (first, oid)
}

pub fn query_style(
    style: &MapCSS,
    cl: &str,
    cltags: &HashMap<String, String>,
    minzoom: i32,
    maxzoom: i32,
) -> Vec<QueryResult> {
    let clname = match cl.find('-') {
        Some(pos) => &cl[..pos],
        None => cl,
    };

    let mut cltags = cltags.clone();
    cltags.insert("name".to_string(), "name".to_string());
    cltags.insert(
        "addr:housenumber".to_string(),
        "addr:housenumber".to_string(),
    );
    cltags.insert("addr:housename".to_string(), "addr:housename".to_string());
    cltags.insert("ref".to_string(), "ref".to_string());
    cltags.insert("int_name".to_string(), "int_name".to_string());
    cltags.insert("addr:flats".to_string(), "addr:flats".to_string());

    let is_area_type = cltags.contains_key("area");
    let mut results = Vec::new();
    let mut chain_cache: ChainMatchCache = HashMap::new();
    for zoom in minzoom..=maxzoom {
        let mut all_runtime_conditions: Vec<Vec<Condition>> = Vec::new();
        if !is_area_type {
            all_runtime_conditions.extend(style.get_runtime_rules(
                clname,
                "line",
                &cltags,
                zoom,
                &mut chain_cache,
            ));
        }
        all_runtime_conditions.extend(style.get_runtime_rules(
            clname,
            "area",
            &cltags,
            zoom,
            &mut chain_cache,
        ));
        if !is_area_type {
            all_runtime_conditions.extend(style.get_runtime_rules(
                clname,
                "node",
                &cltags,
                zoom,
                &mut chain_cache,
            ));
        }

        let mut runtime_conditions_arr: Vec<Option<Vec<Condition>>> = Vec::new();
        if all_runtime_conditions.is_empty() {
            runtime_conditions_arr.push(None);
        } else if all_runtime_conditions.len() == 1 {
            runtime_conditions_arr.push(Some(all_runtime_conditions.remove(0)));
        } else {
            runtime_conditions_arr.push(Some(all_runtime_conditions.remove(0)));
            let mut i = 0;
            while i < all_runtime_conditions.len() {
                let mut conditions_unique = true;
                for rt in runtime_conditions_arr.iter() {
                    if Some(&all_runtime_conditions[i]) == rt.as_ref() {
                        conditions_unique = false;
                        break;
                    }
                }
                if conditions_unique {
                    runtime_conditions_arr.push(Some(all_runtime_conditions[i].clone()));
                }
                i += 1;
            }
        }

        for runtime_conditions in runtime_conditions_arr {
            let mut zstyle: HashMap<String, HashMap<String, StyleValue>> = HashMap::new();
            if !is_area_type {
                zstyle = style.get_style_dict(
                    clname,
                    "line",
                    &cltags,
                    zoom,
                    1.0,
                    0.5,
                    zstyle,
                    runtime_conditions.as_ref(),
                    &mut chain_cache,
                );
            }
            zstyle = style.get_style_dict(
                clname,
                "area",
                &cltags,
                zoom,
                1.0,
                0.5,
                zstyle,
                runtime_conditions.as_ref(),
                &mut chain_cache,
            );
            if !is_area_type {
                zstyle = style.get_style_dict(
                    clname,
                    "node",
                    &cltags,
                    zoom,
                    1.0,
                    0.5,
                    zstyle,
                    runtime_conditions.as_ref(),
                    &mut chain_cache,
                );
            }
            let mut entries: Vec<(String, HashMap<String, StyleValue>)> =
                zstyle.into_iter().collect();
            entries.sort_by_cached_key(|e| rule_sort_key(&e.1));
            let zstyle: Vec<HashMap<String, StyleValue>> =
                entries.into_iter().map(|e| e.1).collect();
            results.push(QueryResult {
                cl: cl.to_string(),
                zoom,
                runtime_conditions,
                zstyle,
            });
        }
    }
    results
}

pub fn get_priorities_filename(prio_range: PrioRangeKind, path: &str) -> String {
    format!(
        "{}/priorities_{}_{}.prio.txt",
        path,
        prio_range.file_pos(),
        prio_range
    )
}

fn repr_prio_key(key: &PrioKey) -> String {
    format!(
        "({}, {})",
        compat::repr_str(&key.cl),
        compat::repr_str(&key.object_id)
    )
}

pub fn load_priorities(
    pipeline: &mut Pipeline,
    prio_range: PrioRangeKind,
    path: &str,
    classif: &HashSet<String>,
    compress: bool,
) {
    let priority_max = if prio_range == PrioRangeKind::Overlays {
        OVERLAYS_MAX_PRIORITY
    } else {
        LAYER_PRIORITY_RANGE
    };
    let priority_min = if prio_range == PrioRangeKind::Overlays {
        -OVERLAYS_MAX_PRIORITY
    } else {
        0
    };
    let fname = get_priorities_filename(prio_range, path);
    let text = std::fs::read_to_string(&fname).unwrap_or_default();
    let mut group: Vec<PrioKey> = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim().to_string();
        if line.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() > 2 {
            println!("WARNING: skipping malformed line in {}:\n\t{}", fname, line);
            continue;
        }
        if tokens[0] == "===" {
            match tokens.get(1).and_then(|s| s.parse::<i32>().ok()) {
                Some(priority) => {
                    if priority >= priority_min && priority < priority_max {
                        if !group.is_empty() {
                            let prios = &mut pipeline.prio_ranges[prio_range].priorities;
                            for key in &group {
                                prios.insert(key.clone(), priority);
                            }
                        } else {
                            println!(
                                "WARNING: skipping empty priority group in {}:\n\t{}",
                                fname, line
                            );
                        }
                    } else {
                        println!(
                            "WARNING: skipping out of [{};{}) range priority value in {}:\n\t{}",
                            priority_min, priority_max, fname, line
                        );
                    }
                }
                None => {
                    println!(
                        "WARNING: skipping invalid priority value in {}:\n\t{}",
                        fname, line
                    );
                }
            }
            group = Vec::new();
        } else {
            let mut cl = tokens[0].to_string();
            let mut object_id = String::new();
            if let Some(oid_pos) = cl.find("::") {
                object_id = cl[oid_pos..].to_string();
                cl = cl[..oid_pos].to_string();
            }
            if !classif.contains(&cl) {
                println!(
                    "WARNING: unknown classificator type in {}:\n\t{}",
                    fname, line
                );
            }
            let key = PrioKey::main(&cl, &object_id);
            if let Some(&prev) = pipeline.prio_ranges[prio_range].priorities.get(&key) {
                println!(
                    "WARNING: overriding previously set priority value {} in {}:\n\t{}",
                    prev, fname, line
                );
            }
            group.push(key);
        }
    }

    if !group.is_empty() {
        let keys: Vec<String> = group.iter().map(repr_prio_key).collect();
        println!(
            "WARNING: skipping last types groups with no priority set in {}:\n\t[{}]",
            fname,
            keys.join(", ")
        );
    }

    if prio_range == PrioRangeKind::Overlays {
        let keys: Vec<PrioKey> = pipeline.prio_ranges[PrioRangeKind::Overlays]
            .priorities
            .keys()
            .cloned()
            .collect();
        for key in keys {
            let mut main_prio_id: Option<PrioKey> = None;
            if key.object_id.starts_with("caption") {
                main_prio_id = Some(PrioKey::main(
                    &key.cl,
                    &key.object_id.replacen("caption", "icon", 1),
                ));
            }
            if key.object_id.starts_with("pathtext") {
                main_prio_id = Some(PrioKey::main(
                    &key.cl,
                    &key.object_id.replacen("pathtext", "shield", 1),
                ));
            }
            if let Some(main_prio_id) = main_prio_id
                && let Some(&main_prio) = pipeline.prio_ranges[PrioRangeKind::Overlays]
                    .priorities
                    .get(&main_prio_id)
            {
                let cur = pipeline.prio_ranges[PrioRangeKind::Overlays].priorities[&key];
                if cur > main_prio {
                    println!(
                        "WARNING: {} priority is higher than {}, making it equal",
                        repr_prio_key(&key),
                        repr_prio_key(&main_prio_id)
                    );
                    pipeline.prio_ranges[PrioRangeKind::Overlays]
                        .priorities
                        .insert(key, main_prio);
                }
            }
        }
    }

    if compress {
        println!(
            "Compressing {} priorities into a (0;{}) range:",
            prio_range, priority_max
        );
        let mut unique_prios: Vec<i32> = pipeline.prio_ranges[prio_range]
            .priorities
            .values()
            .copied()
            .collect();
        unique_prios.sort();
        unique_prios.dedup();
        println!("\tunique priorities values: {}", unique_prios.len());
        let mut base_idx = 1;
        if !unique_prios.contains(&0) {
            base_idx = 0;
            unique_prios.push(0);
        }
        unique_prios.push(priority_max);
        let step = (priority_max as f64 / unique_prios.len() as f64).min(10.0);
        println!("\tnew step between priorities: {}", compat::float_str(step));
        unique_prios.sort();
        let keys: Vec<PrioKey> = pipeline.prio_ranges[prio_range]
            .priorities
            .keys()
            .cloned()
            .collect();
        for prio_id in keys {
            let idx = unique_prios
                .iter()
                .position(|&p| p == pipeline.prio_ranges[prio_range].priorities[&prio_id])
                .unwrap_or(0);
            let new_prio = (step * (base_idx + idx) as f64) as i64;
            pipeline.prio_ranges[prio_range]
                .priorities
                .insert(prio_id, new_prio as i32);
        }
    }
}

pub fn store_visibility(
    pipeline: &mut Pipeline,
    cl: &str,
    object_id: &str,
    dr_type: &str,
    zoom: i32,
    auto_comment: Option<String>,
) {
    let object_id = if object_id == "::default" {
        ""
    } else {
        object_id
    };
    let entries = pipeline.visibilities.entry(cl.to_string()).or_default();
    let mut found: Option<usize> = None;
    for (i, e) in entries.iter().enumerate() {
        if e.dr_type == dr_type && e.auto_comment == auto_comment {
            found = Some(i);
            break;
        }
    }
    let idx = match found {
        Some(i) => i,
        None => {
            entries.push(VisEntry {
                dr_type: dr_type.to_string(),
                auto_comment,
                object_ids: BTreeMap::new(),
            });
            entries.len() - 1
        }
    };
    entries[idx]
        .object_ids
        .entry(object_id.to_string())
        .or_default()
        .insert(zoom);
}

pub fn prettify_zooms(zooms: &BTreeSet<i32>, maxzoom: i32) -> String {
    fn add_zrange(first: i32, last: i32, result: &mut String, maxzoom: i32) {
        let zrange = if last == maxzoom {
            format!("{}-", first)
        } else if first == last {
            first.to_string()
        } else {
            format!("{}-{}", first, last)
        };
        if !result.is_empty() {
            result.push(',');
        }
        result.push_str(&zrange);
    }

    let sorted: Vec<i32> = zooms.iter().copied().collect();
    let mut first = sorted.first().copied().unwrap_or(0);
    let mut prev = first;
    let mut result = String::new();
    for &zoom in &sorted[1.min(sorted.len())..] {
        if zoom == prev + 1 {
            prev = zoom;
        } else {
            add_zrange(first, prev, &mut result, maxzoom);
            first = zoom;
            prev = zoom;
        }
    }
    add_zrange(first, prev, &mut result, maxzoom);
    format!("z{}", result)
}

pub fn validate_visibilities(pipeline: &mut Pipeline, maxzoom: i32) {
    let snapshot = pipeline.visibilities.clone();
    for cl in snapshot.keys() {
        let entries = &snapshot[cl];
        for entry in entries {
            let object_ids: Vec<(String, BTreeSet<i32>)> = entry
                .object_ids
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (object_id, zooms) in object_ids {
                let zoom_range = prettify_zooms(&zooms, maxzoom);
                if zoom_range.contains(',') {
                    println!(
                        "WARNING: non-contiguous visibility range {} for {} {:?}{}",
                        zoom_range, cl, entry.dr_type, object_id
                    );
                }

                if entry.dr_type == "caption"
                    && let Some(icon_entry) = entries
                        .iter()
                        .find(|e| e.dr_type == "icon" && e.auto_comment.is_none())
                    && let Some(icon_zooms) = icon_entry.object_ids.get(&object_id)
                {
                    let mut icon_zooms: Vec<i32> = icon_zooms.iter().copied().collect();
                    icon_zooms.sort();
                    let min_zoom = zooms.iter().next().copied().unwrap_or(0);
                    if min_zoom < icon_zooms[0] {
                        let iz: BTreeSet<i32> = icon_zooms.iter().copied().collect();
                        println!(
                            "WARNING: caption {} appears before icon {} for {}{}",
                            zoom_range,
                            prettify_zooms(&iz, maxzoom),
                            cl,
                            object_id
                        );
                    }
                }

                if entry.dr_type == "pathtext" || entry.dr_type == "shield" {
                    let mut lines_min_zoom = maxzoom + 1;
                    if let Some(line_entry) = entries
                        .iter()
                        .find(|e| e.dr_type == "line" && e.auto_comment.is_none())
                    {
                        for line_zooms in line_entry.object_ids.values() {
                            let min_zoom = line_zooms.iter().next().copied().unwrap_or(0);
                            if min_zoom < lines_min_zoom {
                                lines_min_zoom = min_zoom;
                            }
                        }
                    }
                    let min_zoom = zooms.iter().next().copied().unwrap_or(0);
                    if min_zoom < lines_min_zoom {
                        let missing: BTreeSet<i32> = (min_zoom..lines_min_zoom).collect();
                        println!(
                            "ERROR: {} without line at {} for {}{}",
                            entry.dr_type,
                            prettify_zooms(&missing, maxzoom),
                            cl,
                            object_id
                        );
                        pipeline.validation_errors_count += 1;
                    }
                }
            }
        }
    }
}

pub fn dump_priorities(pipeline: &Pipeline, prio_range: PrioRangeKind, path: &str, maxzoom: i32) {
    use std::io::Write;
    let fname = get_priorities_filename(prio_range, path);
    let mut out = String::new();
    let comment = format!(
        "{}{}{}",
        COMMENT_AUTOFORMAT,
        prio_range.comment(),
        COMMENT_RANGES_OVERVIEW
    );
    for s in comment.lines() {
        let line = format!("# {}", s.trim_end());
        out.push_str(&format!("{}\n", line.trim_end()));
    }
    out.push('\n');

    let prios_map = &pipeline.prio_ranges[prio_range].priorities;
    if !prios_map.is_empty() {
        let dr_types_order: &[&str] = if prio_range == PrioRangeKind::Overlays {
            &["icon", "caption", "pathtext", "shield", "line", "area"]
        } else {
            &["line", "area", "icon", "caption", "pathtext", "shield"]
        };

        let mut prios: Vec<(&PrioKey, &i32)> = prios_map.iter().collect();
        prios.sort_by(|a, b| {
            let ka = (OVERLAYS_MAX_PRIORITY - *a.1, &a.0.cl, &a.0.object_id);
            let kb = (OVERLAYS_MAX_PRIORITY - *b.1, &b.0.cl, &b.0.object_id);
            ka.cmp(&kb)
        });

        let mut comment_auto_captions: Option<&str> = Some(COMMENT_AUTO_CAPTIONS);
        let mut group_prio = *prios[0].1;
        let mut group = String::new();
        let mut group_comment = "# ".to_string();
        for p in &prios {
            if *p.1 != group_prio {
                if prio_range == PrioRangeKind::Overlays
                    && comment_auto_captions.is_some()
                    && group_prio < 0
                {
                    for s in comment_auto_captions.unwrap().lines() {
                        let line = format!("# {}", s.trim());
                        out.push_str(&format!("{}\n", line.trim_end()));
                    }
                    out.push('\n');
                    comment_auto_captions = None;
                }
                out.push_str(&format!("{}{}=== {}\n\n", group, group_comment, group_prio));
                group_prio = *p.1;
                group = String::new();
                group_comment = "# ".to_string();
            }

            let key = p.0;
            let mut cl = key.cl.clone();
            let object_id = key.object_id.clone();
            let mut auto_dr_type: Option<String> = None;
            let mut auto_comment: Option<String> = None;
            if let Some((at, ac)) = &key.auto {
                auto_dr_type = Some(at.clone());
                auto_comment = ac.clone();
            }

            let mut line_drules = String::new();
            let mut other_drules = String::new();
            if let Some(cl_vis) = pipeline.visibilities.get(&key.cl) {
                let mut sorted_entries: Vec<&VisEntry> = cl_vis.iter().collect();
                sorted_entries.sort_by(|a, b| {
                    let ia = dr_types_order
                        .iter()
                        .position(|&t| t == a.dr_type)
                        .unwrap_or(usize::MAX);
                    let ib = dr_types_order
                        .iter()
                        .position(|&t| t == b.dr_type)
                        .unwrap_or(usize::MAX);
                    ia.cmp(&ib)
                });
                for entry in sorted_entries {
                    for (oid, zooms) in &entry.object_ids {
                        let mut dr_zoom = format!("{}{}", entry.dr_type, oid);
                        if let Some(ac) = &entry.auto_comment {
                            dr_zoom = format!("{}({})", dr_zoom, ac);
                        }
                        dr_zoom = format!("{} {}", dr_zoom, prettify_zooms(zooms, maxzoom));

                        let is_auto_dr_match = Some(&entry.dr_type) == auto_dr_type.as_ref()
                            && entry.auto_comment == auto_comment;
                        let is_not_auto_dr = auto_dr_type.is_none() && entry.auto_comment.is_none();
                        let is_suitable_for_range = (prio_range == PrioRangeKind::Overlays
                            && ["icon", "caption", "pathtext", "shield"]
                                .contains(&entry.dr_type.as_str()))
                            || ((prio_range == PrioRangeKind::Fg
                                || prio_range == PrioRangeKind::BgTop)
                                && ["line", "area"].contains(&entry.dr_type.as_str()))
                            || (prio_range == PrioRangeKind::BgBySize && entry.dr_type == "area");

                        if *oid == object_id
                            && (is_auto_dr_match || (is_not_auto_dr && is_suitable_for_range))
                        {
                            if !line_drules.is_empty() {
                                line_drules.push_str(" and ");
                            }
                            line_drules.push_str(&dr_zoom);
                        } else {
                            if !other_drules.is_empty() {
                                other_drules.push_str(", ");
                            }
                            other_drules.push_str(&dr_zoom);
                        }
                    }
                }
            }

            if !object_id.is_empty() {
                cl.push_str(&object_id);
            }
            if line_drules.is_empty() {
                if !other_drules.is_empty() {
                    line_drules = "WARNING: no drule defined for the priority".to_string();
                } else {
                    line_drules =
                        "WARNING: no style defined (the type will be not included into map data)"
                            .to_string();
                }
                println!("{} for {} in {}", line_drules, cl, prio_range);
            }

            let mut info = format!("# {}", line_drules);
            if !other_drules.is_empty() {
                info.push_str(&format!(" (also has {})", other_drules));
            }
            if auto_dr_type.is_none() {
                group_comment = String::new();
            } else {
                cl = format!("# {}", cl);
            }
            group.push_str(&format!("{:<50}  {}\n", cl, info));
        }
        out.push_str(&format!("{}{}=== {}\n", group, group_comment, group_prio));
    }

    let mut file = std::fs::File::create(&fname).unwrap_or_else(|e| {
        panic!("cannot write {}: {}", fname, e);
    });
    file.write_all(out.as_bytes()).unwrap();
}

pub fn get_drape_priority(
    pipeline: &mut Pipeline,
    cl: &str,
    object_id: &str,
    dr_type: &str,
    auto_dr_type: Option<&str>,
    auto_comment: Option<&str>,
    auto_prio_mod: i32,
) -> i32 {
    let object_id = if object_id == "::default" {
        ""
    } else {
        object_id
    };
    let prio_id = PrioKey::main(cl, object_id);

    let ranges_to_check: &[PrioRangeKind] = if dr_type == "line" {
        &[PrioRangeKind::Fg, PrioRangeKind::BgTop]
    } else if dr_type == "area" {
        &[
            PrioRangeKind::BgBySize,
            PrioRangeKind::BgTop,
            PrioRangeKind::Fg,
        ]
    } else {
        &[PrioRangeKind::Overlays]
    };
    for r in ranges_to_check {
        if let Some(&priority) = pipeline.prio_ranges[*r].priorities.get(&prio_id) {
            let mut priority = priority;
            if let Some(auto_dr_type) = auto_dr_type {
                let min_priority = if *r == PrioRangeKind::Overlays {
                    -OVERLAYS_MAX_PRIORITY
                } else {
                    0
                };
                priority = (priority + auto_prio_mod).max(min_priority);
                let auto_prio_id = PrioKey::auto(
                    cl,
                    object_id,
                    auto_dr_type,
                    auto_comment.map(|s| s.to_string()),
                );
                pipeline.prio_ranges[*r]
                    .priorities
                    .insert(auto_prio_id, priority);
            }
            return priority + r.base();
        }
    }

    println!(
        "ERROR: priority is not set for {} {}{}",
        dr_type, cl, object_id
    );
    pipeline.validation_errors_count += 1;
    0
}

/// Reads `colors.txt` (if it exists) into a set of raw colors.
fn load_colors(data_dir: &str) -> HashSet<u32> {
    let mut colors = HashSet::new();
    if let Ok(text) = std::fs::read_to_string(format!("{}/colors.txt", data_dir)) {
        for line in text.lines() {
            if let Ok(c) = line.parse::<u32>() {
                colors.insert(c);
            }
        }
    }
    colors
}

/// Result of `build_classificator`: (classificator, class_order, class_tree,
/// types_lines).
type ClassificatorResult = Result<
    (
        HashMap<String, Vec<(String, String)>>,
        Vec<String>,
        HashMap<String, String>,
        Vec<String>,
    ),
    String,
>;

fn build_classificator(data_dir: &str) -> ClassificatorResult {
    let mut classificator: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut class_order: Vec<String> = Vec::new();
    let mut class_tree: HashMap<String, String> = HashMap::new();
    let mut types_lines: Vec<String> = Vec::new();

    let mut cnt: i64 = 1;
    let mut unique_types_check = HashSet::new();
    let mapping_text = std::fs::read_to_string(format!("{}/mapcss-mapping.csv", data_dir))
        .map_err(|e| format!("cannot read mapcss-mapping.csv: {}", e))?;
    for raw_row in mapping_text.lines() {
        let mut row: Vec<&str> = raw_row.split(';').collect();
        if row.len() <= 1 || row[0].starts_with('#') {
            continue;
        }
        let expr: String;
        if row.len() == 3 {
            let tag = row[0].replace('|', "=");
            let obsolete = !row[2].trim().is_empty();
            expr = format!("[{}]", tag);
            row = vec![
                row[0],
                &expr,
                if obsolete { "x" } else { "" },
                "name",
                "int_name",
                row[1],
                if row[2] != "x" { row[2] } else { "" },
            ];
        }
        if row.len() != 7 {
            return Err(format!(
                "Expecting 3 or 7 columns in mapcss-mapping: {}",
                raw_row
            ));
        }

        let id: i64 = row[5]
            .trim()
            .parse()
            .map_err(|_| format!("Wrong type id: {}", raw_row))?;
        if id < cnt {
            return Err(format!("Wrong type id: {}", raw_row));
        }
        while id > cnt {
            types_lines.push("mapswithme".to_string());
            cnt += 1;
        }
        cnt += 1;

        let cl = row[0].replace('|', "-");
        if unique_types_check.contains(&cl) && row[2] != "x" {
            return Err(format!("Duplicate type: {}", row[0]));
        }
        let mut kv: Vec<(String, String)> = Vec::new();
        {
            let first = row[1].split(',').next().unwrap_or("");
            for i in first.split('[') {
                let i = i.trim_end_matches(']');
                let parts: Vec<&str> = i.split('=').collect();
                if parts.len() == 1 {
                    if !parts[0].is_empty() {
                        if let Some(rest) = parts[0].strip_prefix('!') {
                            kv.push((rest.trim_end_matches('?').to_string(), "no".to_string()));
                        } else {
                            kv.push((
                                parts[0].trim_end_matches('?').to_string(),
                                "yes".to_string(),
                            ));
                        }
                    }
                } else {
                    kv.push((parts[0].to_string(), parts[1].to_string()));
                }
            }
        }
        if row[2] != "x" {
            classificator.insert(cl.clone(), kv);
            class_order.push(cl.clone());
            unique_types_check.insert(cl.clone());
            types_lines.push(format!("*{}", row[0]));
        } else {
            if !row[6].is_empty() {
                types_lines.push(row[6].to_string());
            } else {
                types_lines.push("mapswithme".to_string());
            }
        }
        class_tree.insert(cl, row[0].to_string());
    }
    class_order.sort();
    Ok((classificator, class_order, class_tree, types_lines))
}

fn load_dynamic_tags(data_dir: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Ok(text) = std::fs::read_to_string(format!("{}/mapcss-dynamic.txt", data_dir)) {
        for line in text.lines() {
            if !line.is_empty() {
                set.insert(line.to_string());
            }
        }
    }
    set
}

/// Analyzes a class's zstyle: computes has_lines/has_icons/has_fills and the
/// list of indices of style dicts bearing text (deduplicated by text value).
fn analyze_zstyle(zstyle: &[HashMap<String, StyleValue>]) -> (bool, bool, bool, Vec<usize>) {
    let mut has_lines = false;
    let mut has_icons = false;
    let mut has_fills = false;
    let mut has_text: Vec<usize> = Vec::new();
    let mut txfmt: Vec<String> = Vec::new();
    for (i, st) in zstyle.iter().enumerate() {
        let filtered: Vec<&String> = st.keys().filter(|k| sv_kept_after_strip(&st[*k])).collect();
        if filtered.contains(&&"width".to_string())
            || filtered.contains(&&"pattern-image".to_string())
        {
            has_lines = true;
        }
        if (filtered.contains(&&"icon-image".to_string())
            && st.get("icon-image").map(sv_str).as_deref() != Some("none"))
            || filtered.contains(&&"symbol-image".to_string())
        {
            has_icons = true;
        }
        if filtered.contains(&&"fill-color".to_string())
            && st.get("fill-color").map(sv_str).as_deref() != Some("none")
        {
            has_fills = true;
        }
        if let Some(text) = st.get("text") {
            let text_str = sv_str(text);
            if !text_str.is_empty() && text_str != "none" && !txfmt.contains(&text_str) {
                txfmt.push(text_str);
                has_text.push(i);
            }
        }
    }
    (has_lines, has_icons, has_fills, has_text)
}

fn casing_linecap(st: &HashMap<String, StyleValue>) -> String {
    st.get("casing-linecap")
        .map(sv_str)
        .unwrap_or_else(|| "butt".to_string())
}

fn casing_linejoin(st: &HashMap<String, StyleValue>) -> String {
    st.get("casing-linejoin")
        .map(sv_str)
        .unwrap_or_else(|| "round".to_string())
}

fn cmp_repl(a: &str, b: &str) -> std::cmp::Ordering {
    if a == b {
        return std::cmp::Ordering::Equal;
    }
    let a = a.replace('|', "-");
    let b = b.replace('|', "-");
    if a > b {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Less
    }
}

/// Adds a dash pattern to the `patterns` list unless already present.
fn add_pattern(patterns: &mut Vec<Vec<f64>>, dashes: &[f64]) {
    if !dashes.is_empty() && !patterns.contains(&dashes.to_vec()) {
        patterns.push(dashes.to_vec());
    }
}

pub fn generate_drules(options: &Options, pipeline: &mut Pipeline) -> Result<(), String> {
    let ddir: String = if let Some(d) = &options.data {
        if Path::new(d).is_dir() {
            d.clone()
        } else {
            Path::new(&options.outfile)
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| ".".to_string())
        }
    } else {
        Path::new(&options.outfile)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ".".to_string())
    };

    let mut colors = load_colors(&ddir);

    let mut patterns: Vec<Vec<f64>> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(format!("{}/patterns.txt", ddir)) {
        for line in text.lines() {
            let dashes: Vec<f64> = line
                .split_whitespace()
                .filter_map(|x| x.parse::<f64>().ok())
                .collect();
            add_pattern(&mut patterns, &dashes);
        }
    }

    let (classificator, class_order, class_tree, types_lines) = build_classificator(&ddir)?;
    std::fs::write(format!("{}/types.txt", ddir), types_lines.join("\n") + "\n")
        .map_err(|e| format!("cannot write types.txt: {}", e))?;

    let mut cltags_maps: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (cl, kv) in &classificator {
        cltags_maps.insert(cl.clone(), kv.iter().cloned().collect());
    }

    let mut output = String::new();
    for prio_range in PrioRangeKind::ALL {
        let classif_keys: HashSet<String> = classificator.keys().cloned().collect();
        load_priorities(
            pipeline,
            prio_range,
            &options.priorities_path,
            &classif_keys,
            false,
        );
        if !output.is_empty() {
            output.push_str(", ");
        }
        output.push_str(&format!(
            "{} {}",
            pipeline.prio_ranges[prio_range].priorities.len(),
            prio_range
        ));
    }
    println!("Loaded priorities: {}.", output);

    let mapcss_static_tags: HashMap<String, bool> = {
        let mut tags: HashMap<String, bool> = HashMap::new();
        for kv in classificator.values() {
            for (i, (t, _)) in kv.iter().enumerate() {
                let prev = tags.get(t).copied().unwrap_or(true);
                tags.insert(t.clone(), prev && i == 0);
            }
        }
        tags
    };

    let mapcss_dynamic_tags = load_dynamic_tags(&ddir);

    let mut style = MapCSS::new(options.minzoom, options.maxzoom);
    style.parse(
        None,
        false,
        LAYER_PRIORITY_RANGE as f64,
        options.filename.as_ref().map(Path::new),
        &mapcss_static_tags,
        &mapcss_dynamic_tags,
    )?;

    let mut clname_cltag_unique: HashSet<String> = HashSet::new();
    for cl in &class_order {
        let clname = match cl.find('-') {
            Some(pos) => &cl[..pos],
            None => cl,
        };
        let cltag = classificator[cl]
            .first()
            .map(|kv| kv.0.clone())
            .unwrap_or_default();
        let key = format!("{}${}", clname, cltag);
        if clname_cltag_unique.insert(key) {
            style.build_choosers_tree(clname, "line", &cltag);
            style.build_choosers_tree(clname, "area", &cltag);
            style.build_choosers_tree(clname, "node", &cltag);
        }
    }
    style.finalize_choosers_tree();

    let mut style_colors: HashMap<String, u32> = HashMap::new();
    if let Some(raw_style_colors) = style.get_colors() {
        let mut unique_style_colors: BTreeSet<String> = BTreeSet::new();
        for k in raw_style_colors.keys() {
            if let Some(idx) = k.rfind('-') {
                unique_style_colors.insert(k[..idx].to_string());
            }
        }
        for k in unique_style_colors {
            let v = mwm_encode_color(&mut colors, &raw_style_colors, &k, "black")?;
            style_colors.insert(k, v);
        }
    }

    let dr_linecaps: [(&str, u8); 3] =
        [("none", BUTT_CAP), ("butt", BUTT_CAP), ("round", ROUND_CAP)];
    let dr_linejoins: [(&str, u8); 3] = [
        ("none", NO_JOIN),
        ("bevel", BEVEL_JOIN),
        ("round", ROUND_JOIN),
    ];

    let mut drules = Container::new();
    let mut dr_cont: Option<ClassifElement> = None;
    let mut visstring: Option<Vec<u8>> = None;
    let mut last_cl: String = String::new();
    let mut visibility: HashMap<String, String> = HashMap::new();
    let mut all_draw_elements: HashSet<String> = HashSet::new();

    if !style_colors.is_empty() {
        let mut sorted: Vec<(&String, &u32)> = style_colors.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in sorted {
            let mut color_element = ColorElement::new();
            color_element.set_name(k);
            color_element.set_color(*v);
            drules.colors.value.push(color_element);
        }
    }

    let n_zooms = (options.maxzoom - options.minzoom + 1) as usize;

    for cl in &class_order {
        let results = query_style(
            &style,
            cl,
            &cltags_maps[cl],
            options.minzoom,
            options.maxzoom,
        );
        for result in results {
            let cl = &result.cl;
            let zoom = result.zoom;
            let runtime_conditions = result.runtime_conditions;
            let mut zstyle = result.zstyle;

            zstyle.sort_by_cached_key(rule_sort_key);

            if let Some(dr_cont_ref) = &dr_cont
                && dr_cont_ref.name != *cl
            {
                let viskey = format!("world|{}|", class_tree[&dr_cont_ref.name]);
                let vs = visstring.take().unwrap();
                visibility.insert(viskey, String::from_utf8(vs).unwrap());
                if !dr_cont_ref.element.is_empty() {
                    let cont = dr_cont.take().unwrap();
                    drules.cont.push(cont);
                }
                dr_cont = None;
            }

            if dr_cont.is_none() {
                let mut new_cont = ClassifElement::new();
                new_cont.set_name(cl.clone());
                dr_cont = Some(new_cont);
                visstring = Some(vec![b'0'; n_zooms]);
            }

            if zstyle.is_empty() {
                last_cl = cl.clone();
                continue;
            }

            let (has_lines, mut has_icons, mut has_fills, mut has_text) = analyze_zstyle(&zstyle);

            if !has_lines && has_text.is_empty() && !has_fills && !has_icons {
                last_cl = cl.clone();
                continue;
            }

            if let Some(vs) = visstring.as_mut() {
                vs[zoom as usize] = b'1';
            }

            if zoom == 0 {
                last_cl = cl.clone();
                continue;
            }

            let mut dr_element = DrawElement::new();
            dr_element.set_scale(zoom as u8);

            if let Some(rc) = &runtime_conditions {
                for condition in rc {
                    dr_element.apply_if.push(condition.repr());
                }
            }

            for st in &zstyle {
                let mut casing_width_val: Option<f64> = st.get("casing-width").map(sv_float);
                let has_casing_add = st.get("casing-width-add").is_some();
                let is_area_st = st.contains_key("fill-color");
                let casing_active =
                    casing_width_val.is_some() && casing_width_val != Some(0.0) || has_casing_add;
                if casing_active {
                    if has_lines && !is_area_st && casing_linecap(st) == "butt" {
                        let mut dr_line = LineRule::new();
                        let mut base_width: f64 = st.get("width").map(sv_float).unwrap_or(0.0);
                        if base_width == 0.0 {
                            for wst in &zstyle {
                                let w = wst.get("width").map(sv_float);
                                if let Some(wv) = w
                                    && wv != 0.0
                                    && (base_width == 0.0
                                        || wst.get("object-id").map(sv_str).as_deref()
                                            != Some("::default"))
                                {
                                    base_width = wv;
                                }
                            }
                            let cw = st.get("casing-width");
                            let cw_is_missing = cw.is_none() || cw == Some(&StyleValue::Num(0.0));
                            if cw_is_missing {
                                casing_width_val = Some(
                                    base_width
                                        + st.get("casing-width-add").map(sv_float).unwrap_or(0.0),
                                );
                                base_width = 0.0;
                            }
                        }
                        dr_line.set_width(compat::round2(
                            base_width + casing_width_val.unwrap_or(0.0) * 2.0,
                        ));
                        dr_line.set_color(mwm_encode_color(&mut colors, st, "casing", "black")?);
                        let object_id = st.get("object-id").map(sv_str).unwrap_or_default();
                        if object_id == "::default" {
                            let auto_comment = "casing".to_string();
                            let priority = get_drape_priority(
                                pipeline,
                                cl,
                                &object_id,
                                "line",
                                Some("line"),
                                Some(&auto_comment),
                                -1,
                            );
                            dr_line.set_priority(priority);
                            store_visibility(
                                pipeline,
                                cl,
                                &object_id,
                                "line",
                                zoom,
                                Some(auto_comment),
                            );
                        } else {
                            let priority =
                                get_drape_priority(pipeline, cl, &object_id, "line", None, None, 0);
                            dr_line.set_priority(priority);
                            store_visibility(pipeline, cl, &object_id, "line", zoom, None);
                        }
                        let dashes = st.get("casing-dashes").or_else(|| st.get("dashes"));
                        if let Some(StyleValue::Dash(dd)) = dashes {
                            for d in dd {
                                dr_line.dashdot.dd.push(*d);
                            }
                        }
                        add_pattern(&mut patterns, &dr_line.dashdot.dd);
                        dr_line.set_cap(
                            dr_linecaps
                                .iter()
                                .find(|(k, _)| *k == casing_linecap(st))
                                .map(|(_, v)| *v)
                                .unwrap_or(BUTT_CAP),
                        );
                        dr_line.set_join(
                            dr_linejoins
                                .iter()
                                .find(|(k, _)| *k == casing_linejoin(st))
                                .map(|(_, v)| *v)
                                .unwrap_or(ROUND_JOIN),
                        );
                        dr_element.lines.push(dr_line);
                    }

                    if has_fills
                        && is_area_st
                        && sv_float(st.get("fill-opacity").unwrap_or(&StyleValue::Num(1.0))) > 0.0
                    {
                        dr_element.area.border.set_color(mwm_encode_color(
                            &mut colors,
                            st,
                            "casing",
                            "black",
                        )?);
                        dr_element
                            .area
                            .border
                            .set_width(casing_width_val.unwrap_or(0.0));
                    }
                }

                if has_lines {
                    if let Some(w) = st.get("width")
                        && sv_kept_after_strip(w)
                    {
                        let mut dr_line = LineRule::new();
                        dr_line.set_width(sv_float(w));
                        dr_line.set_color(mwm_encode_color(&mut colors, st, "", "black")?);
                        if let Some(StyleValue::Dash(dd)) = st.get("dashes") {
                            for d in dd {
                                dr_line.dashdot.dd.push(*d);
                            }
                        }
                        add_pattern(&mut patterns, &dr_line.dashdot.dd);
                        let lc = st
                            .get("linecap")
                            .map(sv_str)
                            .unwrap_or_else(|| "butt".to_string());
                        let lj = st
                            .get("linejoin")
                            .map(sv_str)
                            .unwrap_or_else(|| "round".to_string());
                        dr_line.set_cap(
                            dr_linecaps
                                .iter()
                                .find(|(k, _)| *k == lc)
                                .map(|(_, v)| *v)
                                .unwrap_or(BUTT_CAP),
                        );
                        dr_line.set_join(
                            dr_linejoins
                                .iter()
                                .find(|(k, _)| *k == lj)
                                .map(|(_, v)| *v)
                                .unwrap_or(ROUND_JOIN),
                        );
                        let object_id = st.get("object-id").map(sv_str).unwrap_or_default();
                        dr_line.set_priority(get_drape_priority(
                            pipeline, cl, &object_id, "line", None, None, 0,
                        ));
                        store_visibility(pipeline, cl, &object_id, "line", zoom, None);
                        dr_element.lines.push(dr_line);
                    }
                    if let Some(image) = st.get("pattern-image")
                        && sv_kept_after_strip(image)
                    {
                        let mut dr_line = LineRule::new();
                        dr_line.set_width(0.0);
                        dr_line.set_color(0u32);
                        if let Some(handle) = mwm_encode_image(st, "pattern") {
                            dr_line.pathsym.set_name(handle);
                        }
                        dr_line.pathsym.set_step(
                            sv_float(st.get("pattern-spacing").unwrap_or(&StyleValue::Num(0.0)))
                                - 16.0,
                        );
                        dr_line.pathsym.set_offset(sv_float(
                            st.get("pattern-offset").unwrap_or(&StyleValue::Num(0.0)),
                        ));
                        let object_id = st.get("object-id").map(sv_str).unwrap_or_default();
                        dr_line.set_priority(get_drape_priority(
                            pipeline, cl, &object_id, "line", None, None, 0,
                        ));
                        store_visibility(pipeline, cl, &object_id, "line", zoom, None);
                        dr_element.lines.push(dr_line);
                    }
                }

                if let Some(sz) = st.get("shield-font-size")
                    && sv_kept_after_strip(sz)
                {
                    dr_element.shield.set_height(sv_int(sz) as i32);
                    dr_element.shield.set_text_color(mwm_encode_color(
                        &mut colors,
                        st,
                        "shield-text",
                        "black",
                    )?);
                    if sv_float(
                        st.get("shield-text-halo-radius")
                            .unwrap_or(&StyleValue::Num(0.0)),
                    ) != 0.0
                    {
                        dr_element.shield.set_text_stroke_color(mwm_encode_color(
                            &mut colors,
                            st,
                            "shield-text-halo",
                            "white",
                        )?);
                    }
                    dr_element.shield.set_color(mwm_encode_color(
                        &mut colors,
                        st,
                        "shield",
                        "black",
                    )?);
                    if sv_float(
                        st.get("shield-outline-radius")
                            .unwrap_or(&StyleValue::Num(0.0)),
                    ) != 0.0
                    {
                        dr_element.shield.set_stroke_color(mwm_encode_color(
                            &mut colors,
                            st,
                            "shield-outline",
                            "white",
                        )?);
                    }
                    let object_id = st.get("object-id").map(sv_str).unwrap_or_default();
                    dr_element.shield.set_priority(get_drape_priority(
                        pipeline, cl, &object_id, "shield", None, None, 0,
                    ));
                    store_visibility(pipeline, cl, &object_id, "shield", zoom, None);
                    if sv_float(
                        st.get("shield-min-distance")
                            .unwrap_or(&StyleValue::Num(0.0)),
                    ) != 0.0
                    {
                        let md = st.get("shield-min-distance").unwrap();
                        dr_element.shield.set_min_distance(sv_int(md) as i32);
                    }
                }

                if has_icons
                    && let Some(image) = st.get("icon-image")
                    && sv_kept_after_strip(image)
                    && sv_str(image) != "none"
                {
                    if let Some(handle) = mwm_encode_image(st, "icon") {
                        dr_element.symbol.set_name(handle);
                    }
                    let object_id = st.get("object-id").map(sv_str).unwrap_or_default();
                    dr_element.symbol.set_priority(get_drape_priority(
                        pipeline, cl, &object_id, "icon", None, None, 0,
                    ));
                    store_visibility(pipeline, cl, &object_id, "icon", zoom, None);
                    if st.contains_key("icon-min-distance") {
                        let md = st.get("icon-min-distance").unwrap();
                        dr_element.symbol.set_min_distance(sv_int(md) as i32);
                    }
                    has_icons = false;
                }

                if !has_text.is_empty() && st.contains_key("text") {
                    let text_val = sv_str(st.get("text").unwrap());
                    if sv_kept_after_strip(&StyleValue::Str(text_val.clone())) && text_val != "none"
                    {
                        let caption_texts: Vec<usize> = has_text.iter().copied().take(2).collect();

                        let text_position = st
                            .get("text-position")
                            .map(sv_str)
                            .unwrap_or_else(|| "center".to_string());
                        let (mut dr_text, text_priority_key) = if text_position == "line" {
                            (CaptionRule::new(), "pathtext".to_string())
                        } else {
                            (CaptionRule::new(), "caption".to_string())
                        };

                        let mut cur = &mut dr_text.primary;
                        for &sp_idx in &caption_texts {
                            let sp = &zstyle[sp_idx];
                            let font_size = sp
                                .get("font-size")
                                .map(sv_str)
                                .unwrap_or_else(|| "10".to_string());
                            let first = font_size.split(',').next().unwrap_or("10").trim();
                            cur.set_height(first.parse::<f64>().unwrap_or(0.0) as i32);
                            if !st.contains_key("text-color") {
                                println!("ERROR: text-color not set for z{} {}", zoom, cl);
                                pipeline.validation_errors_count += 1;
                            }
                            cur.set_color(mwm_encode_color(&mut colors, sp, "text", "black")?);
                            if sv_float(st.get("text-halo-radius").unwrap_or(&StyleValue::Num(0.0)))
                                != 0.0
                            {
                                cur.set_stroke_color(mwm_encode_color(
                                    &mut colors,
                                    sp,
                                    "text-halo",
                                    "white",
                                )?);
                            }
                            if sp.contains_key("text-offset") || sp.contains_key("text-offset-y") {
                                let offset = sp
                                    .get("text-offset-y")
                                    .or_else(|| sp.get("text-offset"))
                                    .map(sv_int)
                                    .unwrap_or(0);
                                cur.set_offset_y(offset as i32);
                            } else if sp.contains_key("text-offset-x") {
                                let ox = sp.get("text-offset-x").unwrap();
                                cur.set_offset_x(sv_int(ox) as i32);
                            } else if text_position == "center" && dr_element.symbol.priority != 0 {
                                println!(
                                    "ERROR: an icon is present, but caption's text-offset is not set for z{} {}",
                                    zoom, cl
                                );
                                pipeline.validation_errors_count += 1;
                            }
                            if sp.contains_key("text") {
                                let t = sv_str(sp.get("text").unwrap());
                                if t != "name" && t != "int_name" {
                                    cur.set_text(t);
                                }
                            }
                            if sp.contains_key("text-optional") {
                                let (is_valid, value) =
                                    to_boolean(&sv_str(sp.get("text-optional").unwrap()));
                                if is_valid {
                                    cur.set_is_optional(value);
                                } else {
                                    cur.set_is_optional(true);
                                }
                            } else if text_priority_key == "caption"
                                && dr_element.symbol.priority != 0
                            {
                                cur.set_is_optional(true);
                            }
                            cur = &mut dr_text.secondary;
                        }

                        let mut auto_comment: Option<String> = None;
                        let object_id = st.get("object-id").map(sv_str).unwrap_or_default();
                        if text_priority_key == "caption" && dr_element.symbol.priority != 0 {
                            let mut auto_prio_mod = 0;
                            auto_comment = Some("mandatory".to_string());
                            if dr_text.primary.is_optional {
                                auto_comment = Some("optional".to_string());
                                auto_prio_mod = -OVERLAYS_MAX_PRIORITY;
                            }
                            let priority = get_drape_priority(
                                pipeline,
                                cl,
                                &object_id,
                                "icon",
                                Some(&text_priority_key),
                                auto_comment.as_deref(),
                                auto_prio_mod,
                            );
                            dr_text.set_priority(priority);
                        } else {
                            dr_text.set_priority(get_drape_priority(
                                pipeline,
                                cl,
                                &object_id,
                                &text_priority_key,
                                None,
                                None,
                                0,
                            ));
                        }
                        store_visibility(
                            pipeline,
                            cl,
                            &object_id,
                            &text_priority_key,
                            zoom,
                            auto_comment,
                        );

                        if text_position == "line" {
                            dr_element.path_text = dr_text;
                        } else {
                            dr_element.caption = dr_text;
                        }
                        has_text = Vec::new();
                    }
                }

                if has_fills
                    && let Some(fc) = st.get("fill-color")
                    && sv_str(fc) != "none"
                    && sv_float(st.get("fill-opacity").unwrap_or(&StyleValue::Num(1.0))) > 0.0
                {
                    dr_element
                        .area
                        .set_color(mwm_encode_color(&mut colors, st, "fill", "black")?);
                    let object_id = st.get("object-id").map(sv_str).unwrap_or_default();
                    dr_element.area.set_priority(get_drape_priority(
                        pipeline, cl, &object_id, "area", None, None, 0,
                    ));
                    store_visibility(pipeline, cl, &object_id, "area", zoom, None);
                    has_fills = false;
                }
            }

            let name = dr_cont.as_ref().unwrap().name.clone();
            let str_dr_element = format!("{}/{}", name, dr_element.canonical(false));
            if all_draw_elements.insert(str_dr_element) {
                dr_cont.as_mut().unwrap().element.push(dr_element);
            }
            last_cl = cl.clone();
        }
    }

    if let Some(dr_cont_ref) = dr_cont {
        if !dr_cont_ref.element.is_empty() {
            drules.cont.push(dr_cont_ref.clone());
        }
        let viskey = format!("world|{}|", class_tree[&last_cl]);
        if let Some(vs) = visstring {
            visibility.insert(viskey, String::from_utf8(vs).unwrap());
        }
    }

    validate_visibilities(pipeline, options.maxzoom);

    if pipeline.validation_errors_count > 0 {
        return Err(format!(
            "FAILED to write regenerated drules files!\nThere are {} validation errors (see in the log above).\nFix all errors first and re-run.",
            pipeline.validation_errors_count
        ));
    }

    let mut output = String::new();
    for prio_range in PrioRangeKind::ALL {
        dump_priorities(
            pipeline,
            prio_range,
            &options.priorities_path,
            options.maxzoom,
        );
        if !output.is_empty() {
            output.push_str(", ");
        }
        output.push_str(&format!(
            "{} {}",
            pipeline.prio_ranges[prio_range].priorities.len(),
            prio_range
        ));
    }
    println!("Re-formated priorities files: {}.", output);

    let variant = Path::new(&options.outfile)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| options.outfile.clone());
    let bin = serialize_binary(
        std::slice::from_ref(&drules),
        std::slice::from_ref(&variant),
    )?;
    std::fs::write(format!("{}.bin", options.outfile), bin)
        .map_err(|e| format!("cannot write {}.bin: {}", options.outfile, e))?;

    if options.txt {
        let txt = serialize_text(&[drules], &[variant])?;
        std::fs::write(format!("{}.txt", options.outfile), txt)
            .map_err(|e| format!("cannot write {}.txt: {}", options.outfile, e))?;
    }

    let mut visnodes: HashSet<String> = HashSet::new();
    for k in visibility.keys() {
        let vis: Vec<&str> = k.split('|').collect();
        for i in 1..vis.len().saturating_sub(1) {
            visnodes.insert(format!("{}|", vis[0..i].join("|")));
        }
    }
    let mut viskeys: Vec<String> = visibility.keys().cloned().collect();
    for n in &visnodes {
        if !viskeys.contains(n) {
            viskeys.push(n.clone());
        }
    }
    viskeys.sort_by(|a, b| cmp_repl(a, b));

    let mut visibility_lines: Vec<String> = Vec::new();
    let mut classificator_lines: Vec<String> = Vec::new();
    let mut oldoffset = String::new();
    for k in &viskeys {
        let offset = "    ".repeat(k.matches('|').count().saturating_sub(1));
        let old_levels = oldoffset.len() / 4;
        for i in (offset.len() / 4 + 1..=old_levels).rev() {
            visibility_lines.push(format!("{}{{}}", "    ".repeat(i)));
            classificator_lines.push(format!("{}{{}}", "    ".repeat(i)));
        }
        oldoffset = offset.clone();
        let end = if visnodes.contains(k) { "+" } else { "-" };
        let parts: Vec<&str> = k.split('|').collect();
        let name = parts[parts.len().saturating_sub(2)];
        let default = "0".repeat((options.maxzoom + 1) as usize);
        let vis = visibility.get(k).cloned().unwrap_or(default);
        visibility_lines.push(format!("{}{}  {}  {}", offset, name, vis, end));
        classificator_lines.push(format!("{}{}  {}", offset, name, end));
    }
    for i in (1..=(oldoffset.len() / 4)).rev() {
        visibility_lines.push(format!("{}{{}}", "    ".repeat(i)));
        classificator_lines.push(format!("{}{{}}", "    ".repeat(i)));
    }

    std::fs::write(
        format!("{}/visibility.txt", ddir),
        visibility_lines.join("\n") + "\n",
    )
    .map_err(|e| format!("cannot write visibility.txt: {}", e))?;
    std::fs::write(
        format!("{}/classificator.txt", ddir),
        classificator_lines.join("\n") + "\n",
    )
    .map_err(|e| format!("cannot write classificator.txt: {}", e))?;

    let mut colors_sorted: Vec<u32> = colors.iter().copied().collect();
    colors_sorted.sort();
    let colors_txt: String = colors_sorted.iter().map(|c| format!("{}\n", c)).collect();
    std::fs::write(format!("{}/colors.txt", ddir), colors_txt)
        .map_err(|e| format!("cannot write colors.txt: {}", e))?;

    let patterns_txt: String = patterns
        .iter()
        .map(|p| {
            format!(
                "{}\n",
                p.iter()
                    .map(|e| compat::float_str(*e))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect();
    std::fs::write(format!("{}/patterns.txt", ddir), patterns_txt)
        .map_err(|e| format!("cannot write patterns.txt: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drules;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/assets")
            .join(name)
    }

    fn options_for(assets_dir: &Path, outfile: &str, txt: bool, data: Option<String>) -> Options {
        Options {
            filename: Some(assets_dir.join("main.mapcss").display().to_string()),
            minzoom: 0,
            maxzoom: 10,
            outfile: assets_dir.join(outfile).display().to_string(),
            txt,
            priorities_path: assets_dir.join("include").display().to_string(),
            data,
        }
    }

    #[test]
    fn generate_drules_mini() {
        let assets_dir = fixture("case-2-generate-drules-mini");
        let include_dir = assets_dir.join("include");

        // Snapshot the committed golden priorities files: the pipeline re-writes
        // them in place and the output must be byte-identical.
        let mut prio_files: Vec<PathBuf> = std::fs::read_dir(&include_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                name.starts_with("priorities_") && name.ends_with(".prio.txt")
            })
            .collect();
        prio_files.sort();
        let prio_snapshot: Vec<(String, Vec<u8>)> = prio_files
            .iter()
            .map(|p| {
                (
                    p.file_name().unwrap().to_string_lossy().to_string(),
                    std::fs::read(p).unwrap(),
                )
            })
            .collect();

        let options = options_for(&assets_dir, "style_output", true, None);
        let mut pipeline = Pipeline::new();
        generate_drules(&options, &mut pipeline)
            .unwrap_or_else(|e| panic!("generate_drules failed: {}", e));

        // types.txt must contain 1173 lines, 148 of them actual types.
        let types_txt = std::fs::read_to_string(assets_dir.join("types.txt")).unwrap();
        let lines: Vec<&str> = types_txt.lines().map(str::trim).collect();
        assert_eq!(
            lines.len(),
            1173,
            "Generated types.txt file should contain 1173 lines"
        );
        assert_eq!(
            lines.iter().filter(|l| **l != "mapswithme").count(),
            148,
            "Actual types count should be 148 as in mapcss-mapping.csv"
        );

        // style_output.bin must contain 20 types with drawing rules.
        let container = drules::load_container(&assets_dir.join("style_output.bin")).unwrap();
        assert_eq!(
            container.cont.len(),
            20,
            "Generated style_output.bin should contain 20 types with drawing rules"
        );

        // The priorities files must be re-written byte-identically.
        for (name, snapshot) in &prio_snapshot {
            let now = std::fs::read(include_dir.join(name)).unwrap();
            assert_eq!(
                snapshot, &now,
                "{} was not re-formatted byte-identically",
                name
            );
        }

        // Clean up generated files.
        for filename in [
            "classificator.txt",
            "colors.txt",
            "patterns.txt",
            "style_output.bin",
            "style_output.txt",
            "types.txt",
            "visibility.txt",
        ] {
            let _ = std::fs::remove_file(assets_dir.join(filename));
        }
    }

    #[test]
    fn generate_drules_validation_errors() {
        let assets_dir = fixture("case-3-styles-validation");
        let mut options = options_for(&assets_dir, "style_output", false, None);
        options.filename = Some(assets_dir.join("missing.mapcss").display().to_string());
        let mut pipeline = Pipeline::new();
        assert!(generate_drules(&options, &mut pipeline).is_err());
    }
}
