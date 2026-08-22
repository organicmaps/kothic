//! MapCSS parser.
//!
//! `MapCSS` is the top-level parser: it scans a MapCSS stylesheet with a
//! tokenizer (classes, zooms, groups, conditions, objects, declarations,
//! comments, imports, variable assignments) and assembles `StyleChooser`s.
//! It also builds the per-type/zoom/class lookup tree used at render time.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, LazyLock};

use regex::Regex;

use crate::condition::{Condition, parse_condition};
use crate::rule::Rule;
use crate::style_chooser::{ChainMatchCache, StyleChooser, StyleValue};

/// Style keys that make a style dict "useful" for drawing.
pub const NEEDED_KEYS: &[&str] = &[
    "width",
    "casing-width",
    "casing-width-add",
    "fill-color",
    "fill-image",
    "icon-image",
    "text",
    "extrude",
    "background-image",
    "background-color",
    "pattern-image",
    "shield-color",
    "symbol-shape",
];

/// Token kinds tracked between iterations of the parser loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    None,
    Zoom,
    Group,
    Condition,
    Object,
    Declaration,
    VariableSet,
}

static RE_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(?sx)/\*.*?\*/\s*").unwrap());
static RE_CLASS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?sx)([.:]:?[*\w]+)\s*").unwrap());
static RE_ZOOM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?isx)\|\s*z([\d-]+)\s*").unwrap());
static RE_GROUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(?sx),\s*").unwrap());
static RE_CONDITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?sx)\[(.+?)\]\s*").unwrap());
static RE_OBJECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(?sx)(\*|[\w]+)\s*").unwrap());
static RE_DECLARATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?sx)\{(.*?)\}\s*").unwrap());
static RE_IMPORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\A(?sx)@import\("(.+?)"\);\s*"#).unwrap());
static RE_VARIABLE_SET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?isx)@([a-z][\w\d]*)\s*:\s*(.+?)\s*;\s*").unwrap());
static RE_UNKNOWN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(?sx)(\S+)\s*").unwrap());

static RE_ZOOM_MINMAX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(\d+)-(\d+)$").unwrap());
static RE_ZOOM_MIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(\d+)-$").unwrap());
static RE_ZOOM_MAX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A-(\d+)$").unwrap());
static RE_ZOOM_SINGLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(\d+)$").unwrap());

static RE_ASSIGNMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?xs)\A\s*(\S+)\s*:\s*(.+?)\s*$").unwrap());
static RE_VARIABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@([a-z][\w\d]*)").unwrap());

/// Per-class cell in the chooser tree under construction.
struct Cell {
    arr: Vec<usize>,
    set: HashSet<usize>,
}

/// A stack frame used while parsing `@import`ed files.
struct Frame {
    filename: Option<String>,
    original: String,
    /// Byte offset into `original`: everything before it is already consumed.
    /// Token matching works on zero-copy slices of `original`.
    pos: usize,
}

pub struct MapCSS {
    pub minscale: i32,
    pub maxscale: i32,
    pub scalepair: (f64, f64),
    pub choosers: Vec<StyleChooser>,
    /// Choosers grouped by compatible object type (`area`, `line`, `way`,
    /// `node`, `colors`, ...). Stores indices into `choosers` to preserve the
    /// Python object-identity semantics.
    pub choosers_by_type: HashMap<String, Vec<usize>>,
    /// Optimized per-type/zoom/class chooser lists, produced by
    /// `finalize_choosers_tree`.
    pub choosers_by_type_zoom_tag:
        HashMap<String, HashMap<i32, HashMap<String, Vec<StyleChooser>>>>,
    build_cells: HashMap<String, HashMap<i32, HashMap<String, Cell>>>,
    pub variables: HashMap<String, String>,
    pub unused_variables: BTreeSet<String>,
    pub style_loaded: bool,
}

impl MapCSS {
    pub fn new(minscale: i32, maxscale: i32) -> MapCSS {
        MapCSS {
            minscale,
            maxscale,
            scalepair: (minscale as f64, maxscale as f64),
            choosers: Vec::new(),
            choosers_by_type: HashMap::new(),
            choosers_by_type_zoom_tag: HashMap::new(),
            build_cells: HashMap::new(),
            variables: HashMap::new(),
            unused_variables: BTreeSet::new(),
            style_loaded: false,
        }
    }

    pub fn parse_zoom(&self, s: &str) -> Result<(f64, f64), String> {
        if let Some(c) = RE_ZOOM_MINMAX.captures(s) {
            let a: f64 = c.get(1).unwrap().as_str().parse().unwrap();
            let b: f64 = c.get(2).unwrap().as_str().parse().unwrap();
            return Ok((a, b));
        }
        if let Some(c) = RE_ZOOM_MIN.captures(s) {
            let a: f64 = c.get(1).unwrap().as_str().parse().unwrap();
            return Ok((a, self.maxscale as f64));
        }
        if let Some(c) = RE_ZOOM_MAX.captures(s) {
            let b: f64 = c.get(1).unwrap().as_str().parse().unwrap();
            return Ok((self.minscale as f64, b));
        }
        if let Some(c) = RE_ZOOM_SINGLE.captures(s) {
            let a: f64 = c.get(1).unwrap().as_str().parse().unwrap();
            return Ok((a, a));
        }
        Err(format!("unparsed zoom: {}", s))
    }

    pub fn build_choosers_tree(&mut self, clname: &str, type_: &str, cltag: &str) {
        let cells = self.build_cells.entry(type_.to_string()).or_default();
        for zoom in self.minscale..=self.maxscale {
            let cmap = cells.entry(zoom).or_default();
            cmap.entry(clname.to_string()).or_insert_with(|| Cell {
                arr: Vec::new(),
                set: HashSet::new(),
            });
        }
        if let Some(by_type) = self.choosers_by_type.get(type_) {
            let idxs = by_type.clone();
            for i in idxs {
                let (chooser_tags, selzooms) = {
                    let chooser = &mut self.choosers[i];
                    let tags = chooser.extract_tags();
                    (tags, chooser.selzooms)
                };
                if chooser_tags.contains("*") || chooser_tags.contains(cltag) {
                    let mut chosen = Vec::new();
                    if let Some((lo, hi)) = selzooms {
                        for zoom in lo as i32..=hi as i32 {
                            chosen.push(zoom);
                        }
                    }
                    for zoom in chosen {
                        let cell = self
                            .build_cells
                            .get_mut(type_)
                            .unwrap()
                            .get_mut(&zoom)
                            .unwrap()
                            .get_mut(clname)
                            .unwrap();
                        if !cell.set.contains(&i) {
                            cell.arr.push(i);
                            cell.set.insert(i);
                        }
                    }
                }
            }
        }
    }

    pub fn finalize_choosers_tree(&mut self) {
        let cells = std::mem::take(&mut self.build_cells);
        let mut tree: HashMap<String, HashMap<i32, HashMap<String, Vec<StyleChooser>>>> =
            HashMap::new();
        // One type-filtered rule-chains allocation per (original chooser,
        // type); every zoom copy for that cell shares it, so the chain-match
        // cache keyed on the `Arc` pointer stays valid across zooms.
        let mut chains_by_type: HashMap<(usize, String), Arc<Vec<Rule>>> = HashMap::new();
        for (ftype, zmap) in cells {
            for (zoom, cmap) in zmap {
                for (clname, cell) in cmap {
                    let mut arr: Vec<StyleChooser> = Vec::with_capacity(cell.arr.len());
                    for &i in &cell.arr {
                        let chooser = &self.choosers[i];
                        let key = (i, ftype.clone());
                        let chains = chains_by_type.entry(key).or_insert_with(|| {
                            Arc::new(
                                chooser
                                    .rule_chains
                                    .iter()
                                    .filter(|rule| rule.type_matches.iter().any(|m| m == &ftype))
                                    .cloned()
                                    .collect(),
                            )
                        });
                        let mut optimized = StyleChooser::new(chooser.scalepair);
                        optimized.styles = Arc::clone(&chooser.styles);
                        optimized.has_evals = chooser.has_evals;
                        optimized.has_runtime_conditions = chooser.has_runtime_conditions;
                        optimized.selzooms = Some((zoom as f64, zoom as f64));
                        optimized.rule_chains = Arc::clone(chains);
                        arr.push(optimized);
                    }
                    tree.entry(ftype.clone())
                        .or_default()
                        .entry(zoom)
                        .or_default()
                        .insert(clname, arr);
                }
            }
        }
        self.choosers_by_type_zoom_tag = tree;
    }

    pub fn get_runtime_rules(
        &self,
        clname: &str,
        type_: &str,
        tags: &HashMap<String, String>,
        zoom: i32,
        cache: &mut ChainMatchCache,
    ) -> Vec<Vec<Condition>> {
        let mut runtime_rules = Vec::new();
        if let Some(t) = self.choosers_by_type_zoom_tag.get(type_)
            && let Some(z) = t.get(&zoom)
            && let Some(c) = z.get(clname)
        {
            for chooser in c {
                runtime_rules.extend(chooser.get_runtime_conditions(tags, cache));
            }
        }
        runtime_rules
    }

    pub fn get_style(
        &self,
        clname: &str,
        type_: &str,
        tags: &HashMap<String, String>,
        zoom: i32,
        xscale: f64,
        zscale: f64,
        filter_by_runtime_conditions: Option<&Vec<Condition>>,
        cache: &mut ChainMatchCache,
    ) -> Vec<HashMap<String, StyleValue>> {
        let mut style: Vec<HashMap<String, StyleValue>> = Vec::new();
        if let Some(t) = self.choosers_by_type_zoom_tag.get(type_)
            && let Some(z) = t.get(&zoom)
            && let Some(c) = z.get(clname)
        {
            for chooser in c {
                chooser.apply_styles(
                    &mut style,
                    tags,
                    xscale,
                    zscale,
                    filter_by_runtime_conditions,
                    cache,
                );
            }
        }
        style.retain(|x| !matches!(x.get("object-id"), Some(StyleValue::Str(s)) if s == "::*"));
        for x in style.iter_mut() {
            for k in ["width", "casing-width"] {
                if x.get(k) == Some(&StyleValue::Num(0.0)) {
                    x.remove(k);
                }
            }
        }
        style.retain(|x| NEEDED_KEYS.iter().any(|k| x.contains_key(*k)));
        style
    }

    pub fn get_colors(&self) -> Option<HashMap<String, StyleValue>> {
        let colors = self.choosers_by_type.get("colors")?;
        let i = colors.first()?;
        self.choosers[*i].styles.first().cloned()
    }

    pub fn get_style_dict(
        &self,
        clname: &str,
        type_: &str,
        tags: &HashMap<String, String>,
        zoom: i32,
        xscale: f64,
        zscale: f64,
        olddict: HashMap<String, HashMap<String, StyleValue>>,
        filter_by_runtime_conditions: Option<&Vec<Condition>>,
        cache: &mut ChainMatchCache,
    ) -> HashMap<String, HashMap<String, StyleValue>> {
        let r = self.get_style(
            clname,
            type_,
            tags,
            zoom,
            xscale,
            zscale,
            filter_by_runtime_conditions,
            cache,
        );
        let mut d = olddict;
        for x in r {
            let oid = x
                .get("object-id")
                .cloned()
                .unwrap_or(StyleValue::Str(String::new()));
            let key = match oid {
                StyleValue::Str(s) => s,
                other => other.py_str(),
            };
            let entry = d.entry(key).or_default();
            for (k, v) in x {
                entry.insert(k, v);
            }
        }
        d
    }

    fn subst_variables(&mut self, t: &mut [HashMap<String, String>]) -> Result<(), String> {
        for v in t[0].values_mut() {
            let mut result = String::new();
            let mut last = 0;
            let mut err: Option<String> = None;
            for caps in RE_VARIABLE.captures_iter(v) {
                let m = caps.get(0).unwrap();
                result.push_str(&v[last..m.start()]);
                let name = &m.as_str()[1..];
                self.unused_variables.remove(name);
                if !self.variables.contains_key(name) {
                    err = Some(format!("Variable not found: {}", name));
                    break;
                }
                result.push_str(&self.variables[name]);
                last = m.end();
            }
            if let Some(e) = err {
                return Err(e);
            }
            result.push_str(&v[last..]);
            *v = result;
        }
        Ok(())
    }

    fn wrap_error(&self, msg: &str, frame: &Frame, css: &str) -> String {
        let consumed = frame.original.len() - css.len();
        let line = frame.original[..consumed].matches('\n').count() + 1;
        let fname = frame.filename.as_deref().unwrap_or("None");
        format!("{}\nFile: {}\nLine: {}", msg, fname, line)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn parse(
        &mut self,
        css: Option<&str>,
        clamp: bool,
        stretch: f64,
        filename: Option<&Path>,
        static_tags: &HashMap<String, bool>,
        dynamic_tags: &HashSet<String>,
    ) -> Result<(), String> {
        let basepath: std::path::PathBuf = filename
            .and_then(|f| f.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let css_str: String = match css {
            Some(s) => s.to_string(),
            None => {
                let f = filename.ok_or("filename is required when css is None")?;
                std::fs::read_to_string(f).map_err(|e| e.to_string())?
            }
        };
        if !self.style_loaded {
            self.choosers = Vec::new();
        }

        let mut stck: Vec<Frame> = vec![Frame {
            filename: filename.map(|p| p.display().to_string()),
            original: css_str,
            pos: 0,
        }];

        let mut previous = TokenKind::None;
        let mut sc = StyleChooser::new(self.scalepair);
        let mut had_main_tag = false;

        while !stck.is_empty() {
            // Skip the leading whitespace of the current frame. Every token
            // regex below consumes its own trailing whitespace, so this is the
            // only place that has to handle it explicitly.
            {
                let f = stck.last_mut().unwrap();
                f.pos += f.original[f.pos..].len() - f.original[f.pos..].trim_start().len();
            }
            // The remaining unconsumed text: a zero-copy suffix of `original`.
            let mut css: &str = {
                let f = stck.last().unwrap();
                &f.original[f.pos..]
            };
            let mut was_broken = false;
            while !css.is_empty() {
                if let Some(c) = RE_CLASS.captures(css) {
                    if previous == TokenKind::Declaration {
                        self.choosers.push(sc);
                        sc = StyleChooser::new(self.scalepair);
                    }
                    let cond = c.get(1).unwrap().as_str().to_string();
                    sc.add_condition(Condition::new("eq", vec!["::class".to_string(), cond]));
                    previous = TokenKind::Condition;
                    css = &css[c.get(0).unwrap().end()..];
                } else if let Some(c) = RE_ZOOM.captures(css) {
                    // A zoom directly after an object, condition or declaration
                    // narrows the current rule chain instead of starting a new one.
                    if !matches!(
                        previous,
                        TokenKind::None
                            | TokenKind::Condition
                            | TokenKind::Object
                            | TokenKind::Declaration
                    ) {
                        sc.new_object("");
                    }
                    let cond = c.get(1).unwrap().as_str().to_string();
                    let z = self.parse_zoom(&cond).map_err(|e| {
                        let frame = stck.last().unwrap();
                        self.wrap_error(&e, frame, css)
                    })?;
                    sc.add_zoom(z);
                    previous = TokenKind::Zoom;
                    css = &css[c.get(0).unwrap().end()..];
                } else if let Some(m) = RE_GROUP.find(css) {
                    had_main_tag = false;
                    previous = TokenKind::Group;
                    css = &css[m.end()..];
                } else if let Some(c) = RE_CONDITION.captures(css) {
                    if previous == TokenKind::Declaration {
                        self.choosers.push(sc);
                        sc = StyleChooser::new(self.scalepair);
                        had_main_tag = false;
                    }
                    if previous != TokenKind::Object
                        && previous != TokenKind::Zoom
                        && previous != TokenKind::Condition
                    {
                        sc.new_object("");
                        had_main_tag = false;
                    }
                    let cond = c.get(1).unwrap().as_str().to_string();
                    let parsed = parse_condition(&cond).map_err(|e| {
                        let frame = stck.last().unwrap();
                        self.wrap_error(&e, frame, css)
                    })?;
                    let tag = parsed.extract_tag();
                    let tag_type = static_tags.get(&tag).copied();
                    if tag == "*" || tag_type.is_some() {
                        if tag_type.unwrap_or(false) && had_main_tag {
                            if cond.contains('!') {
                                let cond2 = cond.replace('!', "");
                                sc.add_runtime_condition(Condition::new(
                                    "ne",
                                    vec!["extra_tag".to_string(), cond2],
                                ));
                            } else {
                                sc.add_runtime_condition(Condition::new(
                                    "eq",
                                    vec!["extra_tag".to_string(), cond.clone()],
                                ));
                            }
                        } else {
                            sc.add_condition(parsed);
                            if tag_type.unwrap_or(false) {
                                had_main_tag = true;
                            }
                        }
                    } else if dynamic_tags.contains(&tag) {
                        sc.add_runtime_condition(parsed);
                    } else {
                        let msg = format!("Unknown tag '{}' in condition {}", tag, cond);
                        let frame = stck.last().unwrap();
                        return Err(self.wrap_error(&msg, frame, css));
                    }
                    previous = TokenKind::Condition;
                    css = &css[c.get(0).unwrap().end()..];
                } else if let Some(c) = RE_OBJECT.captures(css) {
                    if previous == TokenKind::Declaration {
                        self.choosers.push(sc);
                        sc = StyleChooser::new(self.scalepair);
                    }
                    let obj = c.get(1).unwrap().as_str().to_string();
                    sc.new_object(&obj);
                    had_main_tag = false;
                    previous = TokenKind::Object;
                    css = &css[c.get(0).unwrap().end()..];
                } else if let Some(c) = RE_DECLARATION.captures(css) {
                    if previous == TokenKind::Declaration || previous == TokenKind::None {
                        let msg = "Declaration without conditions".to_string();
                        let frame = stck.last().unwrap();
                        return Err(self.wrap_error(&msg, frame, css));
                    }
                    let decl = c.get(1).unwrap().as_str().to_string();
                    let mut parsed = parse_declaration(&decl);
                    self.subst_variables(&mut parsed).map_err(|e| {
                        let frame = stck.last().unwrap();
                        self.wrap_error(&e, frame, css)
                    })?;
                    sc.add_styles(parsed);
                    previous = TokenKind::Declaration;
                    css = &css[c.get(0).unwrap().end()..];
                } else if let Some(m) = RE_COMMENT.find(css) {
                    css = &css[m.end()..];
                } else if let Some(c) = RE_IMPORT.captures(css) {
                    let import_filename = basepath.join(c.get(1).unwrap().as_str());
                    let abs_pos = stck.last().unwrap().original.len() - css.len();
                    let consumed_end = abs_pos + c.get(0).unwrap().end();
                    let import_text = std::fs::read_to_string(&import_filename).map_err(|e| {
                        format!("Cannot import file {}\n{}", import_filename.display(), e)
                    })?;
                    // Save the current position and descend into the imported file.
                    stck.last_mut().unwrap().pos = consumed_end;
                    stck.push(Frame {
                        filename: Some(import_filename.display().to_string()),
                        original: import_text,
                        pos: 0,
                    });
                    was_broken = true;
                    break;
                } else if let Some(c) = RE_VARIABLE_SET.captures(css) {
                    let name = c.get(1).unwrap().as_str().to_string();
                    let value = c.get(2).unwrap().as_str().to_string();
                    self.variables.insert(name.clone(), value);
                    self.unused_variables.insert(name);
                    previous = TokenKind::VariableSet;
                    css = &css[c.get(0).unwrap().end()..];
                } else if let Some(c) = RE_UNKNOWN.captures(css) {
                    let msg = format!("Unknown construction: {}", c.get(1).unwrap().as_str());
                    let frame = stck.last().unwrap();
                    return Err(self.wrap_error(&msg, frame, css));
                } else {
                    let frame = stck.last().unwrap();
                    return Err(self.wrap_error("Unexpected construction:", frame, css));
                }
            }
            if !was_broken {
                stck.pop();
            }
        }

        if previous == TokenKind::Declaration {
            self.choosers.push(sc);
        }

        // Clamp z-indexes so they tightly follow integers.
        if clamp {
            let mut zindex: Vec<f64> = Vec::new();
            for chooser in &self.choosers {
                for stylez in chooser.styles.iter() {
                    let zi = zindex_value(stylez.get("z-index"));
                    if !zindex.contains(&zi) {
                        zindex.push(zi);
                    }
                }
            }
            zindex.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for chooser in &mut self.choosers {
                for stylez in Arc::make_mut(&mut chooser.styles) {
                    if let Some(zi) = stylez.get("z-index") {
                        let val = zindex_value(Some(zi));
                        let res = zindex.iter().position(|x| *x == val).unwrap_or(0) as f64;
                        if stretch != 0.0 {
                            stylez.insert(
                                "z-index".to_string(),
                                StyleValue::Num(stretch * res / zindex.len() as f64),
                            );
                        } else {
                            stylez.insert("z-index".to_string(), StyleValue::Num(res));
                        }
                    }
                }
            }
        }

        // Group MapCSS styles by object type: 'area', 'line', 'way', 'node'.
        for (i, chooser) in self.choosers.iter().enumerate() {
            for t in &chooser.compatible_types {
                self.choosers_by_type.entry(t.clone()).or_default().push(i);
            }
        }

        if !self.unused_variables.is_empty() {
            let names: Vec<&str> = self.unused_variables.iter().map(|s| s.as_str()).collect();
            println!("Warning: Unused variables: {}", names.join(", "));
        }

        Ok(())
    }
}

/// Reads a `z-index` style value as a float.
fn zindex_value(v: Option<&StyleValue>) -> f64 {
    match v {
        Some(StyleValue::Num(n)) => *n,
        Some(StyleValue::Str(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

pub fn parse_declaration(s: &str) -> Vec<HashMap<String, String>> {
    let mut t: HashMap<String, String> = HashMap::new();
    for a in s.split(';') {
        if let Some(c) = RE_ASSIGNMENT.captures(a) {
            let key = c.get(1).unwrap().as_str().to_string();
            let value = c
                .get(2)
                .unwrap()
                .as_str()
                .trim()
                .trim_matches('"')
                .to_string();
            t.insert(key, value);
        }
    }
    vec![t]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(map: &[(&str, &str)]) -> HashMap<String, String> {
        map.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn static_tags(map: &[&str]) -> HashMap<String, bool> {
        map.iter().map(|k| (k.to_string(), true)).collect()
    }

    #[test]
    fn test_parse_zoom() {
        let mc = MapCSS::new(0, 19);
        assert_eq!(mc.parse_zoom("6-9").unwrap(), (6.0, 9.0));
        assert_eq!(mc.parse_zoom("6-").unwrap(), (6.0, 19.0));
        assert_eq!(mc.parse_zoom("-6").unwrap(), (0.0, 6.0));
        assert_eq!(mc.parse_zoom("6").unwrap(), (6.0, 6.0));
        assert_eq!(mc.parse_zoom("10-13").unwrap(), (10.0, 13.0));
    }

    #[test]
    fn test_parse_import_midfile() {
        // The import is preceded by comments (one of them multi-line, with
        // selector-looking junk inside): tokens consumed before an import
        // must not shift the position the parent file resumes at.
        let mut parser = MapCSS::new(0, 19);
        let file = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/assets/case-4-import-midfile/main.mapcss"
        );
        parser
            .parse(
                None,
                true,
                1000.0,
                Some(Path::new(file)),
                &HashMap::new(),
                &HashSet::new(),
            )
            .unwrap();
        // Parsing continued past the import point: the rule defined after it
        // (and the variable set after the rule) must have been consumed.
        assert_eq!(parser.choosers.len(), 1);
    }

    #[test]
    fn test_declarations() {
        let decl = parse_declaration(" linejoin: round; ");
        assert_eq!(decl.len(), 1);
        assert_eq!(
            decl[0],
            HashMap::from([("linejoin".to_string(), "round".to_string())])
        );

        let decl = parse_declaration("\tlinejoin :\nround ; ");
        assert_eq!(decl.len(), 1);
        assert_eq!(
            decl[0],
            HashMap::from([("linejoin".to_string(), "round".to_string())])
        );

        let decl = parse_declaration(" icon-image: parking_private-s.svg; text: \"name\"; ");
        assert_eq!(decl.len(), 1);
        assert_eq!(
            decl[0],
            HashMap::from([
                (
                    "icon-image".to_string(),
                    "parking_private-s.svg".to_string()
                ),
                ("text".to_string(), "name".to_string()),
            ])
        );

        let decl = parse_declaration(
            "\n    pattern-offset: 90\t;\n    pattern-image:\tarrow-m.svg   ;\n    pattern-spacing: @trunk0 ;",
        );
        assert_eq!(decl.len(), 1);
        assert_eq!(
            decl[0],
            HashMap::from([
                ("pattern-offset".to_string(), "90".to_string()),
                ("pattern-image".to_string(), "arrow-m.svg".to_string()),
                ("pattern-spacing".to_string(), "@trunk0".to_string()),
            ])
        );
    }

    #[test]
    fn test_parse_variables() {
        let mut parser = MapCSS::new(0, 19);
        parser
            .parse(
                Some("@city_label: #999999;\n@country_label: #444444;\n@wave_length: 25;\n"),
                true,
                1000.0,
                None,
                &HashMap::new(),
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(
            parser.variables,
            HashMap::from([
                ("city_label".to_string(), "#999999".to_string()),
                ("country_label".to_string(), "#444444".to_string()),
                ("wave_length".to_string(), "25".to_string()),
            ])
        );
    }

    #[test]
    fn test_parse_basic_chooser() {
        let mut parser = MapCSS::new(0, 19);
        let st = static_tags(&["tourism", "office", "craft", "amenity"]);
        parser
            .parse(
                Some(
                    "node|z17-[tourism],\narea|z17-[tourism],\nnode|z18-[office],\narea|z18-[office],\nnode|z18-[craft],\narea|z18-[craft],\nnode|z19-[amenity],\narea|z19-[amenity],\n{text: name; text-color: #000030; text-offset: 1;}\n",
                ),
                true,
                1000.0,
                None,
                &st,
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(parser.choosers.len(), 1);
        assert_eq!(parser.choosers[0].rule_chains.len(), 8);
    }

    #[test]
    fn test_parse_basic_chooser_2() {
        let mut parser = MapCSS::new(0, 19);
        let st = static_tags(&["highway"]);
        parser
            .parse(
                Some(
                    "@trunk0: #FF7326;\n\nline|z6[highway=trunk],\nline|z6[highway=motorway],\n{color: @trunk0; opacity: 0.3;}\nline|z7-9[highway=trunk],\nline|z7-9[highway=motorway],\n{color: @trunk0; opacity: 0.7;}\n",
                ),
                true,
                1000.0,
                None,
                &st,
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(parser.choosers.len(), 2);
        assert_eq!(parser.choosers[0].rule_chains.len(), 2);
        assert_eq!(parser.choosers[0].rule_chains[0].subject, "line");
        assert_eq!(parser.choosers[0].selzooms, Some((6.0, 6.0)));
        assert_eq!(parser.choosers[1].selzooms, Some((7.0, 9.0)));

        let tt = parser.choosers[0].rule_chains[0].test(&tags(&[("highway", "trunk")]));
        assert_eq!(tt, Some("::default".to_string()));
    }

    #[test]
    fn test_parse_basic_chooser_3() {
        let mut parser = MapCSS::new(0, 19);
        let st = HashMap::from([
            ("addr:housenumber".to_string(), true),
            ("addr:street".to_string(), false),
        ]);
        parser
            .parse(
                Some(
                    "/* Some Comment Here */\n\n/*\n   This sample is borrowed from Organic Maps Basemap_label.mapcss file\n */\nnode|z18-[addr:housenumber][addr:street]::int_name\n{text: int_name; text-color: #65655E; text-position: center;}\n",
                ),
                true,
                1000.0,
                None,
                &st,
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(parser.choosers.len(), 1);
        let style_chooser = &parser.choosers[0];
        assert_eq!(style_chooser.rule_chains.len(), 1);
        assert_eq!(style_chooser.selzooms, Some((18.0, 19.0)));

        let rule = &style_chooser.rule_chains[0];
        assert_eq!(
            rule.test(&tags(&[
                ("building", "yes"),
                ("addr:housenumber", "12"),
                ("addr:street", "Baker street"),
            ])),
            Some("::int_name".to_string())
        );
        assert_eq!(rule.subject, "node");
        assert_eq!(
            rule.extract_tags(),
            BTreeSet::from(["addr:housenumber".to_string(), "addr:street".to_string()])
        );
    }

    #[test]
    fn test_parse_basic_chooser_class() {
        let mut parser = MapCSS::new(0, 19);
        parser
            .parse(
                Some("way|z-13::*\n{\n  linejoin: round;\n}\n"),
                true,
                1000.0,
                None,
                &HashMap::new(),
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(parser.choosers.len(), 1);
        let style_chooser = &parser.choosers[0];
        assert_eq!(style_chooser.rule_chains.len(), 1);
        assert_eq!(style_chooser.selzooms, Some((0.0, 13.0)));

        let rule = &style_chooser.rule_chains[0];
        assert_eq!(rule.test(&HashMap::new()), Some("::*".to_string()));
        assert_eq!(rule.subject, "way");
        assert_eq!(rule.extract_tags(), BTreeSet::from(["*".to_string()]));
    }

    #[test]
    fn test_parse_basic_chooser_colors() {
        let mut parser = MapCSS::new(0, 19);
        parser
            .parse(
                Some(
                    "way|z-6::*\n{\n  linejoin: round;\n}\n\ncolors {\n  GuiText-color: #FFFFFF;\n  GuiText-opacity: 0.7;\n  MyPositionAccuracy-color: #FFFFFF;\n  MyPositionAccuracy-opacity: 0.06;\n  Selection-color: #FFFFFF;\n  Selection-opacity: 0.64;\n  Route-color: #0000FF;\n  RouteOutline-color: #00FFFF;\n}\n",
                ),
                true,
                1000.0,
                None,
                &HashMap::new(),
                &HashSet::new(),
            )
            .unwrap();
        let colors = parser.get_colors().unwrap();
        assert_eq!(
            colors.get("GuiText-color"),
            Some(&StyleValue::Color((1.0, 1.0, 1.0)))
        );
        assert_eq!(colors.get("GuiText-opacity"), Some(&StyleValue::Num(0.7)));
        assert_eq!(
            colors.get("Route-color"),
            Some(&StyleValue::Color((0.0, 0.0, 1.0)))
        );
        assert_eq!(
            colors.get("RouteOutline-color"),
            Some(&StyleValue::Color((0.0, 1.0, 1.0)))
        );
        assert_eq!(
            colors.get("Selection-opacity"),
            Some(&StyleValue::Num(0.64))
        );
    }

    #[test]
    fn test_parser_choosers_tree() {
        let mut parser = MapCSS::new(0, 19);
        let st = static_tags(&["tourism", "office", "craft", "amenity"]);
        parser
            .parse(
                Some(
                    "node|z17-[office=lawyer],\narea|z17-[office=lawyer],\n{text: name;text-color: #444444;text-offset: 1;font-size: 10;}\n\nnode|z17-[tourism],\narea|z17-[tourism],\nnode|z18-[office],\narea|z18-[office],\nnode|z18-[craft],\narea|z18-[craft],\nnode|z19-[amenity],\narea|z19-[amenity],\n{text: name; text-color: #000030; text-offset: 1;}\n\nnode|z18-[office],\narea|z18-[office],\nnode|z18-[craft],\narea|z18-[craft],\n{font-size: 11;}\n\nnode|z17-[office=lawyer],\narea|z17-[office=lawyer]\n{icon-image: lawyer-m.svg;}\n",
                ),
                true,
                1000.0,
                None,
                &st,
                &HashSet::new(),
            )
            .unwrap();

        for obj_type in ["line", "area", "node"] {
            for cl in ["tourism", "office", "craft", "amenity"] {
                parser.build_choosers_tree(cl, obj_type, cl);
            }
        }
        parser.finalize_choosers_tree();

        let mut cache = ChainMatchCache::new();
        let styles18 = parser.get_style(
            "office",
            "node",
            &tags(&[("office", "lawyer")]),
            18,
            1.0,
            1.0,
            Some(&Vec::new()),
            &mut cache,
        );
        assert_eq!(styles18.len(), 1);
        let s18 = &styles18[0];
        assert_eq!(
            s18.get("object-id"),
            Some(&StyleValue::Str("::default".to_string()))
        );
        assert_eq!(
            s18.get("font-size"),
            Some(&StyleValue::Str("11".to_string()))
        );
        assert_eq!(s18.get("text"), Some(&StyleValue::Str("name".to_string())));
        assert_eq!(
            s18.get("text-color"),
            Some(&StyleValue::Color((0.0, 0.0, 16.0 * 3.0 / 255.0)))
        );
        assert_eq!(s18.get("text-offset"), Some(&StyleValue::Num(1.0)));
        assert_eq!(
            s18.get("icon-image"),
            Some(&StyleValue::Str("lawyer-m.svg".to_string()))
        );

        let mut cache = ChainMatchCache::new();
        let styles17 = parser.get_style(
            "office",
            "node",
            &tags(&[("office", "lawyer")]),
            17,
            1.0,
            1.0,
            Some(&Vec::new()),
            &mut cache,
        );
        assert_eq!(styles17.len(), 1);
        let s17 = &styles17[0];
        assert_eq!(
            s17.get("font-size"),
            Some(&StyleValue::Str("10".to_string()))
        );
        assert_eq!(
            s17.get("text-color"),
            Some(&StyleValue::Color((
                68.0 / 255.0,
                68.0 / 255.0,
                68.0 / 255.0
            )))
        );

        let mut cache = ChainMatchCache::new();
        let styles15 = parser.get_style(
            "office",
            "node",
            &tags(&[("office", "lawyer")]),
            15,
            1.0,
            1.0,
            Some(&Vec::new()),
            &mut cache,
        );
        assert!(styles15.is_empty());
    }

    #[test]
    fn test_parser_choosers_tree_with_classes() {
        let mut parser = MapCSS::new(0, 19);
        let st = static_tags(&["highway"]);
        parser
            .parse(
                Some(
                    "line|z10-[highway=motorway]::shield,\nline|z10-[highway=trunk]::shield,\nline|z10-[highway=motorway_link]::shield,\nline|z10-[highway=trunk_link]::shield,\nline|z10-[highway=primary]::shield,\nline|z11-[highway=primary_link]::shield,\nline|z12-[highway=secondary]::shield,\nline|z13-[highway=tertiary]::shield,\nline|z15-[highway=residential]::shield,\n{\n  shield-font-size: 9;\n  shield-text-color: #000000;\n  shield-text-halo-radius: 0;\n  shield-color: #FFFFFF;\n  shield-outline-radius: 1;\n}\n\nline|z12-[highway=residential],\nline|z12-[highway=tertiary],\nline|z18-[highway=tertiary_link]\n{\n  text: name;\n  text-color: #333333;\n  text-halo-opacity: 0.8;\n  text-halo-radius: 1;\n}\n\nline|z12-13[highway=residential],\nline|z12-13[highway=tertiary]\n{\n    font-size: 12;\n    text-color: #444444;\n}\n",
                ),
                true,
                1000.0,
                None,
                &st,
                &HashSet::new(),
            )
            .unwrap();

        parser.build_choosers_tree("highway", "line", "highway");
        parser.finalize_choosers_tree();

        let mut cache = ChainMatchCache::new();
        let styles10 = parser.get_style(
            "highway",
            "line",
            &tags(&[("highway", "primary")]),
            10,
            1.0,
            1.0,
            Some(&Vec::new()),
            &mut cache,
        );
        assert_eq!(styles10.len(), 1);
        let s10 = &styles10[0];
        assert_eq!(
            s10.get("object-id"),
            Some(&StyleValue::Str("::shield".to_string()))
        );
        assert_eq!(
            s10.get("shield-font-size"),
            Some(&StyleValue::Str("9".to_string()))
        );
        assert_eq!(
            s10.get("shield-text-color"),
            Some(&StyleValue::Color((0.0, 0.0, 0.0)))
        );
        assert_eq!(
            s10.get("shield-text-halo-radius"),
            Some(&StyleValue::Num(0.0))
        );
        assert_eq!(
            s10.get("shield-color"),
            Some(&StyleValue::Color((1.0, 1.0, 1.0)))
        );
        assert_eq!(
            s10.get("shield-outline-radius"),
            Some(&StyleValue::Num(1.0))
        );

        let mut cache = ChainMatchCache::new();
        let styles15 = parser.get_style(
            "highway",
            "line",
            &tags(&[("highway", "tertiary")]),
            15,
            1.0,
            1.0,
            Some(&Vec::new()),
            &mut cache,
        );
        assert_eq!(styles15.len(), 2);
        assert_eq!(
            styles15[0].get("object-id"),
            Some(&StyleValue::Str("::shield".to_string()))
        );
        assert_eq!(
            styles15[1].get("object-id"),
            Some(&StyleValue::Str("::default".to_string()))
        );
        assert_eq!(
            styles15[1].get("text"),
            Some(&StyleValue::Str("name".to_string()))
        );
        assert_eq!(
            styles15[1].get("text-color"),
            Some(&StyleValue::Color((
                51.0 / 255.0,
                51.0 / 255.0,
                51.0 / 255.0
            )))
        );
        assert_eq!(
            styles15[1].get("text-halo-opacity"),
            Some(&StyleValue::Num(0.8))
        );
        assert_eq!(
            styles15[1].get("text-halo-radius"),
            Some(&StyleValue::Num(1.0))
        );
    }

    #[test]
    fn test_parse_import() {
        let mut parser = MapCSS::new(0, 19);
        let file = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/assets/case-1-import/main.mapcss"
        );
        parser
            .parse(
                None,
                true,
                1000.0,
                Some(Path::new(file)),
                &HashMap::new(),
                &HashSet::new(),
            )
            .unwrap();
        let colors = parser.get_colors().unwrap();
        assert_eq!(
            colors.get("GuiText-color"),
            Some(&StyleValue::Color((1.0, 1.0, 1.0)))
        );
        assert_eq!(colors.get("GuiText-opacity"), Some(&StyleValue::Num(0.7)));
        assert_eq!(
            colors.get("Route-color"),
            Some(&StyleValue::Color((0.0, 0.0, 1.0)))
        );
        assert_eq!(colors.get("Route-opacity"), Some(&StyleValue::Num(0.5)));
    }

    #[test]
    fn test_variable_substitution() {
        let mut parser = MapCSS::new(0, 19);
        let st = static_tags(&["highway"]);
        parser
            .parse(
                Some(
                    "@trunk0: #FF7326;\nline|z6[highway=trunk],\n{color: @trunk0; opacity: 0.3;}\n",
                ),
                true,
                1000.0,
                None,
                &st,
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(parser.choosers.len(), 1);
        let style = &parser.choosers[0].styles[0];
        assert_eq!(
            style.get("color"),
            Some(&StyleValue::Color((1.0, 115.0 / 255.0, 38.0 / 255.0)))
        );
        // The variable must no longer be reported as unused.
        assert!(!parser.unused_variables.contains("trunk0"));
    }
}
