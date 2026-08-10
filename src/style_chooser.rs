//! Style chooser.
//!
//! A `StyleChooser` holds one CSS selector+declaration group: `rule_chains`
//! holds the selectors (each a `Rule`), `styles` holds the resulting style
//! dictionaries.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::color::{CairoColor, cairo_to_hex, parse_color_cairo};
use crate::compat;
use crate::condition::Condition;
use crate::eval::{Eval, Val};
use crate::rule::Rule;

/// A value inside a style dictionary.
#[derive(Clone, Debug, PartialEq)]
pub enum StyleValue {
    Str(String),
    Num(f64),
    Color(CairoColor),
    Dash(Vec<f64>),
    Eval(Eval),
    None,
}

impl StyleValue {
    pub fn py_str(&self) -> String {
        match self {
            StyleValue::Str(s) => s.clone(),
            StyleValue::Num(n) => compat::float_str(*n),
            StyleValue::Color(c) => format!("({}, {}, {})", c.0, c.1, c.2),
            StyleValue::Dash(v) => format!(
                "[{}]",
                v.iter()
                    .map(|x| compat::float_str(*x))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            StyleValue::Eval(e) => e.repr(),
            StyleValue::None => "None".to_string(),
        }
    }

    fn to_val(&self) -> Val {
        match self {
            StyleValue::Str(s) => Val::Str(s.clone()),
            StyleValue::Num(n) => Val::Num(*n),
            StyleValue::Color(_) => Val::None,
            StyleValue::Dash(_) => Val::None,
            StyleValue::Eval(_) => Val::None,
            StyleValue::None => Val::None,
        }
    }
}

/// Converts a style dict of raw strings into the "nicified" form used at
/// render time. Port of `make_nice_style`.
pub fn make_nice_style(r: HashMap<String, StyleValue>) -> HashMap<String, StyleValue> {
    let mut ra = HashMap::new();
    for (a, b) in r {
        match b {
            StyleValue::Eval(_) => {
                ra.insert(a, b);
            }
            StyleValue::Str(s) if a.contains("color") && s.trim() != "none" => {
                if !s.is_empty() {
                    if let Some(c) = parse_color_cairo(&s) {
                        ra.insert(a, StyleValue::Color(c));
                    } else {
                        ra.insert(a, StyleValue::Str(s));
                    }
                }
            }
            StyleValue::Str(s)
                if a.contains("width")
                    || a.contains("opacity")
                    || a.contains("offset")
                    || a.contains("radius")
                    || a.contains("extrude") =>
            {
                match s.trim().parse::<f64>() {
                    Ok(n) => {
                        ra.insert(a, StyleValue::Num(n));
                    }
                    Err(_) => {
                        ra.insert(a, StyleValue::Str(s));
                    }
                }
            }
            StyleValue::Str(s) if a.contains("dashes") => {
                let parsed: Result<Vec<f64>, _> =
                    s.split(',').map(|x| x.trim().parse::<f64>()).collect();
                match parsed {
                    Ok(v) => {
                        ra.insert(a, StyleValue::Dash(v));
                    }
                    Err(_) => {
                        ra.insert(a, StyleValue::Dash(Vec::new()));
                    }
                }
            }
            other => {
                ra.insert(a, other);
            }
        }
    }
    ra
}

pub struct StyleChooser {
    /// The selector's rule chains. `Arc` so the per-zoom copies produced by
    /// `finalize_choosers_tree` share one allocation instead of each deep-cloning
    /// it; the copies only differ in their zoom window (`selzooms`).
    pub rule_chains: Arc<Vec<Rule>>,
    /// The selector's declaration groups, shared the same way as `rule_chains`.
    pub styles: Arc<Vec<HashMap<String, StyleValue>>>,
    pub scalepair: (f64, f64),
    pub selzooms: Option<(f64, f64)>,
    pub compatible_types: BTreeSet<String>,
    pub has_evals: bool,
    pub has_runtime_conditions: bool,
    pub cached_tags: Option<BTreeSet<String>>,
}

/// Memo of all matching rule chains per original `StyleChooser`, scoped to one
/// `query_style` call. The chain match depends on the tags only (not on the
/// zoom), and every per-zoom optimized copy of a chooser shares the same
/// `rule_chains` `Arc` (per object type), so the result is computed once per
/// (chooser, type, tags) triple. Deduplication by ::object-id is *not* done
/// here because which chain wins for an object-id depends on the zoom window.
pub type ChainMatchCache = HashMap<usize, Vec<(usize, String)>>;

/// Runs `chooser.test_chains_all(tags)`, caching the result per shared
/// `rule_chains` allocation (one per original chooser and object type) so the
/// tag-only match is not repeated for every zoom level.
pub(crate) fn cached_chain_matches(
    chooser: &StyleChooser,
    tags: &HashMap<String, String>,
    cache: &mut ChainMatchCache,
) -> Vec<(usize, String)> {
    let key = Arc::as_ptr(&chooser.rule_chains) as usize;
    if let Some(res) = cache.get(&key) {
        return res.clone();
    }
    let res: Vec<(usize, String)> = chooser
        .rule_chains
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r.test(tags).map(|tt| (i, tt)))
        .collect();
    cache.insert(key, res.clone());
    res
}

/// Keeps the first entry per distinct ::object-id.
fn first_per_object_id(pairs: impl Iterator<Item = (usize, String)>) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (i, tt) in pairs {
        if !out.iter().any(|(_, t)| t == &tt) {
            out.push((i, tt));
        }
    }
    out
}

impl StyleChooser {
    /// Returns the first matching rule for *each* distinct ::object-id of the
    /// selector group at the chooser's zoom level, so that a declaration block
    /// applies to every layer it selects instead of just the first one.
    ///
    /// The tag match itself is zoom-independent and is resolved once per
    /// original chooser via the cache; zoom applicability (each rule's
    /// min/max zoom range) is then checked on the cached result before
    /// picking the winning chain per ::object-id.
    pub fn chain_matches_at_zoom(
        &self,
        tags: &HashMap<String, String>,
        cache: &mut ChainMatchCache,
    ) -> Vec<(usize, String)> {
        let all = cached_chain_matches(self, tags, cache);
        let Some((zoom, _)) = self.selzooms else {
            return first_per_object_id(all.into_iter());
        };
        first_per_object_id(all.into_iter().filter(|(i, _)| {
            let rule = &self.rule_chains[*i];
            rule.min_zoom <= zoom && zoom <= rule.max_zoom
        }))
    }

    /// Like `chain_matches_at_zoom()`, but without the zoom filter. Used where
    /// no zoom window applies.
    pub fn test_chains_all(&self, tags: &HashMap<String, String>) -> Vec<(usize, String)> {
        first_per_object_id(
            self.rule_chains
                .iter()
                .enumerate()
                .filter_map(|(i, r)| r.test(tags).map(|tt| (i, tt))),
        )
    }
}

impl StyleChooser {
    pub fn new(scalepair: (f64, f64)) -> StyleChooser {
        StyleChooser {
            rule_chains: Arc::new(Vec::new()),
            styles: Arc::new(Vec::new()),
            scalepair,
            selzooms: None,
            compatible_types: BTreeSet::new(),
            has_evals: false,
            has_runtime_conditions: false,
            cached_tags: None,
        }
    }

    pub fn extract_tags(&mut self) -> BTreeSet<String> {
        if let Some(t) = &self.cached_tags {
            return t.clone();
        }
        let mut a = BTreeSet::new();
        for r in self.rule_chains.iter() {
            let mut tags = r.extract_tags();
            a.append(&mut tags);
            if a.contains("*") {
                a = BTreeSet::from(["*".to_string()]);
                break;
            }
        }
        if self.has_evals && !a.contains("*") {
            for s in self.styles.iter() {
                for v in s.values() {
                    if let StyleValue::Eval(ev) = v {
                        let mut tags = ev.extract_tags();
                        a.append(&mut tags);
                    }
                }
            }
        }
        if a.is_empty() {
            a = BTreeSet::from(["*".to_string()]);
        }
        self.cached_tags = Some(a.clone());
        a
    }

    /// Returns the runtime conditions of every ::object-id this chooser selects,
    /// to match the chain_matches_at_zoom() contract used by apply_styles().
    pub fn get_runtime_conditions(
        &self,
        tags: &HashMap<String, String>,
        cache: &mut ChainMatchCache,
    ) -> Vec<Vec<Condition>> {
        if !self.has_runtime_conditions {
            return Vec::new();
        }
        self.chain_matches_at_zoom(tags, cache)
            .iter()
            .filter_map(|(rule_idx, _)| self.rule_chains[*rule_idx].runtime_conditions.clone())
            .collect()
    }

    pub fn apply_styles(
        &self,
        sl: &mut Vec<HashMap<String, StyleValue>>,
        tags: &HashMap<String, String>,
        xscale: f64,
        zscale: f64,
        filter_by_runtime_conditions: Option<&Vec<Condition>>,
        cache: &mut ChainMatchCache,
    ) {
        // Are any of the ruleChains fulfilled?
        for (rule_idx, object_id) in self.chain_matches_at_zoom(tags, cache) {
            self.apply_styles_to(
                sl,
                tags,
                xscale,
                zscale,
                filter_by_runtime_conditions,
                rule_idx,
                &object_id,
            );
        }
    }

    fn apply_styles_to(
        &self,
        sl: &mut Vec<HashMap<String, StyleValue>>,
        tags: &HashMap<String, String>,
        xscale: f64,
        zscale: f64,
        filter_by_runtime_conditions: Option<&Vec<Condition>>,
        rule_idx: usize,
        object_id: &str,
    ) {
        let rule = &self.rule_chains[rule_idx];
        if let (Some(filter), Some(rc)) = (filter_by_runtime_conditions, &rule.runtime_conditions)
            && filter != rc
        {
            return;
        }

        let combined: Option<HashMap<String, StyleValue>> = if self.has_evals {
            let mut combined = HashMap::new();
            for t in sl.iter() {
                combined.extend(t.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            for (p, q) in combined.iter_mut() {
                if p.contains("color")
                    && let StyleValue::Color(c) = q
                {
                    *q = StyleValue::Str(cairo_to_hex(*c));
                }
            }
            Some(combined)
        } else {
            None
        };

        for r in self.styles.iter() {
            let mut ra = if let Some(combined) = &combined {
                let props: HashMap<&str, Val> = combined
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.to_val()))
                    .collect();
                let mut ra = HashMap::new();
                for (a, b) in r {
                    let b = match b {
                        StyleValue::Eval(ev) => {
                            StyleValue::Str(ev.compute(tags, &props, xscale, zscale))
                        }
                        other => other.clone(),
                    };
                    ra.insert(a.clone(), b);
                }
                make_nice_style(ra)
            } else {
                r.clone()
            };

            ra.insert(
                "object-id".to_string(),
                StyleValue::Str(object_id.to_string()),
            );
            let mut hasall = false;
            let mut allinit: HashMap<String, StyleValue> = HashMap::new();
            let mut matched = false;
            for x in sl.iter_mut() {
                if matches!(x.get("object-id"), Some(StyleValue::Str(s)) if s == "::*") {
                    allinit = x.clone();
                }
                if matches!(ra.get("object-id"), Some(StyleValue::Str(s)) if s == "::*") {
                    let oid = x.get("object-id").cloned().unwrap_or(StyleValue::None);
                    for (k, v) in ra.iter() {
                        x.insert(k.clone(), v.clone());
                    }
                    x.insert("object-id".to_string(), oid);
                    if matches!(x.get("object-id"), Some(StyleValue::Str(s)) if s == "::*") {
                        hasall = true;
                    }
                } else {
                    if x.get("object-id") == ra.get("object-id") {
                        for (k, v) in ra.iter() {
                            x.insert(k.clone(), v.clone());
                        }
                        matched = true;
                        break;
                    }
                }
            }
            if !matched && !hasall {
                for (k, v) in ra.iter() {
                    allinit.insert(k.clone(), v.clone());
                }
                sl.push(allinit);
            }
        }
    }

    pub fn new_object(&mut self, e: &str) {
        let mut rule = Rule::new(e);
        rule.min_zoom = self.scalepair.0;
        rule.max_zoom = self.scalepair.1;
        Arc::make_mut(&mut self.rule_chains).push(rule);
    }

    pub fn add_zoom(&mut self, z: (f64, f64)) {
        if let Some(rule) = Arc::make_mut(&mut self.rule_chains).last_mut() {
            rule.min_zoom = z.0;
            rule.max_zoom = z.1;
        }
    }

    pub fn add_condition(&mut self, c: Condition) {
        if let Some(rule) = Arc::make_mut(&mut self.rule_chains).last_mut() {
            rule.conditions.push(c);
        }
    }

    pub fn add_runtime_condition(&mut self, c: Condition) {
        if let Some(rule) = Arc::make_mut(&mut self.rule_chains).last_mut() {
            if rule.runtime_conditions.is_none() {
                rule.runtime_conditions = Some(vec![c]);
                self.has_runtime_conditions = true;
            } else if let Some(rc) = rule.runtime_conditions.as_mut() {
                rc.push(c);
            }
        }
    }

    pub fn add_styles(&mut self, a: Vec<HashMap<String, String>>) {
        for r in self.rule_chains.iter() {
            match &mut self.selzooms {
                Some(sz) => {
                    sz.0 = sz.0.min(r.min_zoom);
                    sz.1 = sz.1.max(r.max_zoom);
                }
                None => self.selzooms = Some((r.min_zoom, r.max_zoom)),
            }
            for t in r.get_compatible_types() {
                self.compatible_types.insert(t);
            }
        }
        let mut rb = Vec::new();
        for r in a {
            let mut ra: HashMap<String, StyleValue> = HashMap::new();
            for (a, b) in r {
                let a = a.trim().to_string();
                let mut b = b.trim().to_string();
                if a == "casing-width"
                    && let Some(c0) = b.chars().next()
                    && c0 == '+'
                    && let Ok(n) = b.trim_start_matches('+').parse::<f64>()
                {
                    b = compat::float_str(n / 2.0);
                }
                if b.starts_with("eval(") {
                    self.has_evals = true;
                    ra.insert(a, StyleValue::Eval(Eval::new(&b)));
                } else {
                    ra.insert(a, StyleValue::Str(b));
                }
            }
            rb.push(make_nice_style(ra));
        }
        Arc::make_mut(&mut self.styles).extend(rb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::parse_condition;

    fn tags(map: &[(&str, &str)]) -> HashMap<String, String> {
        map.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn str_style(map: &[(&str, &str)]) -> HashMap<String, String> {
        map.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_rules_chain() {
        let mut sc = StyleChooser::new((0.0, 16.0));

        sc.new_object("");
        sc.add_condition(parse_condition("highway=footway").unwrap());
        sc.add_condition(parse_condition("footway=sidewalk").unwrap());

        sc.new_object("");
        sc.add_condition(parse_condition("highway=footway").unwrap());
        sc.add_condition(parse_condition("footway=crossing").unwrap());
        sc.add_condition(Condition::new(
            "eq",
            vec!["::class".to_string(), "::*".to_string()],
        ));

        assert!(
            sc.test_chains_all(&tags(&[("highway", "footway")]))
                .is_empty()
        );
        assert!(
            sc.test_chains_all(&tags(&[
                ("highway", "residential"),
                ("footway", "crossing")
            ]))
            .is_empty()
        );

        let (rule1, tt) = sc
            .test_chains_all(&tags(&[("highway", "footway"), ("footway", "sidewalk")]))
            .remove(0);
        assert_eq!(tt, "::default");

        let (rule2, tt) = sc
            .test_chains_all(&tags(&[("highway", "footway"), ("footway", "crossing")]))
            .remove(0);
        assert_eq!(tt, "::*");

        assert_ne!(rule1, rule2);
    }

    #[test]
    fn test_zoom() {
        let mut sc = StyleChooser::new((0.0, 16.0));

        sc.new_object("");
        sc.add_zoom((10.0, 19.0));
        sc.add_condition(parse_condition("railway=station").unwrap());
        sc.add_condition(parse_condition("transport=subway").unwrap());
        sc.add_condition(parse_condition("city=yerevan").unwrap());

        sc.new_object("");
        sc.add_zoom((4.0, 15.0));
        sc.add_condition(parse_condition("railway=station").unwrap());
        sc.add_condition(parse_condition("transport=subway").unwrap());
        sc.add_condition(parse_condition("city=yokohama").unwrap());

        let (rule1, _) = sc
            .test_chains_all(&tags(&[
                ("railway", "station"),
                ("transport", "subway"),
                ("city", "yerevan"),
            ]))
            .remove(0);
        assert_eq!(rule1, 0);
        assert_eq!(sc.rule_chains[rule1].min_zoom, 10.0);
        assert_eq!(sc.rule_chains[rule1].max_zoom, 19.0);

        let (rule2, _) = sc
            .test_chains_all(&tags(&[
                ("railway", "station"),
                ("transport", "subway"),
                ("city", "yokohama"),
            ]))
            .remove(0);
        assert_eq!(rule2, 1);
        assert_eq!(sc.rule_chains[rule2].min_zoom, 4.0);
        assert_eq!(sc.rule_chains[rule2].max_zoom, 15.0);
    }

    #[test]
    fn test_extract_tags() {
        let mut sc = StyleChooser::new((0.0, 16.0));

        sc.new_object("");
        sc.add_condition(parse_condition("aerialway=rope_tow").unwrap());

        sc.new_object("");
        sc.add_condition(parse_condition("piste:type=downhill").unwrap());

        assert_eq!(
            sc.extract_tags(),
            BTreeSet::from(["aerialway".to_string(), "piste:type".to_string()])
        );

        let mut sc = StyleChooser::new((0.0, 16.0));

        sc.new_object("");
        sc.add_condition(parse_condition("aeroway=terminal").unwrap());
        sc.add_condition(parse_condition("building").unwrap());

        sc.new_object("");
        sc.add_condition(parse_condition("waterway=dam").unwrap());
        sc.add_condition(parse_condition("building:part").unwrap());

        assert_eq!(
            sc.extract_tags(),
            BTreeSet::from([
                "waterway".to_string(),
                "building:part".to_string(),
                "building".to_string(),
                "aeroway".to_string()
            ])
        );
    }

    #[test]
    fn test_make_nice_style() {
        let mut style = HashMap::new();
        style.insert(
            "outline-color".to_string(),
            StyleValue::Str("none".to_string()),
        );
        style.insert("bg-color".to_string(), StyleValue::Str("red".to_string()));
        style.insert(
            "dash-color".to_string(),
            StyleValue::Str("#ffff00".to_string()),
        );
        style.insert(
            "front-color".to_string(),
            StyleValue::Str("rgb(0, 255, 255)".to_string()),
        );
        style.insert(
            "line-width".to_string(),
            StyleValue::Eval(Eval::new("eval(min(tag(\"line_width\"), 10))")),
        );
        style.insert(
            "outline-width".to_string(),
            StyleValue::Str("2.5".to_string()),
        );
        style.insert(
            "arrow-opacity".to_string(),
            StyleValue::Str("0.5".to_string()),
        );
        style.insert("offset-2".to_string(), StyleValue::Str("20".to_string()));
        style.insert(
            "border-radius".to_string(),
            StyleValue::Str("4".to_string()),
        );
        style.insert(
            "line-extrude".to_string(),
            StyleValue::Str("16".to_string()),
        );
        style.insert(
            "dashes".to_string(),
            StyleValue::Str("3,3,1.5,3".to_string()),
        );
        style.insert(
            "wrong-dashes".to_string(),
            StyleValue::Str("yes, yes, yes, no".to_string()),
        );
        style.insert("make-nice".to_string(), StyleValue::Str("True".to_string()));
        style.insert("additional-len".to_string(), StyleValue::Num(44.5));

        let style = make_nice_style(style);

        assert_eq!(
            style.get("outline-color"),
            Some(&StyleValue::Str("none".to_string()))
        );
        assert_eq!(
            style.get("bg-color"),
            Some(&StyleValue::Color((1.0, 0.0, 0.0)))
        );
        assert_eq!(
            style.get("dash-color"),
            Some(&StyleValue::Color((1.0, 1.0, 0.0)))
        );
        assert_eq!(
            style.get("front-color"),
            Some(&StyleValue::Color((0.0, 1.0, 1.0)))
        );
        assert_eq!(
            style.get("line-width"),
            Some(&StyleValue::Eval(Eval::new(
                "eval(min(tag(\"line_width\"), 10))"
            )))
        );
        assert_eq!(style.get("outline-width"), Some(&StyleValue::Num(2.5)));
        assert_eq!(style.get("arrow-opacity"), Some(&StyleValue::Num(0.5)));
        assert_eq!(style.get("offset-2"), Some(&StyleValue::Num(20.0)));
        assert_eq!(style.get("border-radius"), Some(&StyleValue::Num(4.0)));
        assert_eq!(style.get("line-extrude"), Some(&StyleValue::Num(16.0)));
        assert_eq!(
            style.get("dashes"),
            Some(&StyleValue::Dash(vec![3.0, 3.0, 1.5, 3.0]))
        );
        assert_eq!(
            style.get("wrong-dashes"),
            Some(&StyleValue::Dash(Vec::new()))
        );
        assert_eq!(
            style.get("make-nice"),
            Some(&StyleValue::Str("True".to_string()))
        );
        assert_eq!(style.get("additional-len"), Some(&StyleValue::Num(44.5)));
    }

    #[test]
    fn test_add_styles() {
        let mut sc = StyleChooser::new((15.0, 19.0));
        sc.new_object("");
        sc.add_styles(vec![str_style(&[
            ("width", "1.3"),
            ("opacity", "0.6"),
            ("bg-color", "blue"),
        ])]);
        sc.add_styles(vec![str_style(&[
            ("color", "#FFFFFF"),
            ("casing-width", "+10"),
        ])]);

        assert_eq!(sc.styles.len(), 2);
        assert_eq!(
            sc.styles[0],
            HashMap::from([
                ("width".to_string(), StyleValue::Num(1.3)),
                ("opacity".to_string(), StyleValue::Num(0.6)),
                ("bg-color".to_string(), StyleValue::Color((0.0, 0.0, 1.0))),
            ])
        );
        assert_eq!(
            sc.styles[1],
            HashMap::from([
                ("color".to_string(), StyleValue::Color((1.0, 1.0, 1.0))),
                ("casing-width".to_string(), StyleValue::Num(5.0)),
            ])
        );
    }

    #[test]
    fn test_update_styles() {
        let mut styles = vec![HashMap::from([(
            "primary_color".to_string(),
            StyleValue::Color((1.0, 1.0, 1.0)),
        )])];

        let mut sc = StyleChooser::new((15.0, 19.0));
        sc.new_object("");
        sc.add_styles(vec![str_style(&[
            ("width", "1.3"),
            ("opacity", "0.6"),
            ("bg-color", "eval( prop(\"primary_color\") )"),
            (
                "text-offset",
                "eval( cond( boolean(tag(\"oneway\")), 10, 5) )",
            ),
        ])]);

        let object_tags = tags(&[("highway", "service"), ("oneway", "yes")]);
        let mut cache = ChainMatchCache::new();
        sc.apply_styles(&mut styles, &object_tags, 1.0, 1.0, None, &mut cache);
        let expected_new_styles = HashMap::from([
            ("width".to_string(), StyleValue::Num(1.3)),
            ("opacity".to_string(), StyleValue::Num(0.6)),
            ("bg-color".to_string(), StyleValue::Color((1.0, 1.0, 1.0))),
            ("text-offset".to_string(), StyleValue::Num(10.0)),
            (
                "object-id".to_string(),
                StyleValue::Str("::default".to_string()),
            ),
        ]);

        assert_eq!(styles.len(), 2);
        assert_eq!(styles.last().unwrap(), &expected_new_styles);
    }

    #[test]
    fn test_update_styles_2() {
        let mut styles: Vec<HashMap<String, StyleValue>> = Vec::new();

        let mut sc = StyleChooser::new((15.0, 19.0));

        sc.new_object("");
        sc.add_condition(Condition::new(
            "eq",
            vec!["::class".to_string(), "::int_name".to_string()],
        ));
        sc.add_condition(parse_condition("oneway?").unwrap());

        sc.add_styles(vec![str_style(&[("width", "1.3"), ("bg-color", "black")])]);

        let object_tags = tags(&[("highway", "service"), ("oneway", "yes")]);
        let mut cache = ChainMatchCache::new();
        sc.apply_styles(&mut styles, &object_tags, 1.0, 1.0, None, &mut cache);
        let expected_new_styles = HashMap::from([
            ("width".to_string(), StyleValue::Num(1.3)),
            ("bg-color".to_string(), StyleValue::Color((0.0, 0.0, 0.0))),
            (
                "object-id".to_string(),
                StyleValue::Str("::int_name".to_string()),
            ),
        ]);

        assert_eq!(styles.len(), 1);
        assert_eq!(styles.last().unwrap(), &expected_new_styles);
    }

    #[test]
    fn test_update_styles_by_class() {
        let mut styles = vec![
            HashMap::from([
                ("some-width".to_string(), StyleValue::Num(2.5)),
                (
                    "object-id".to_string(),
                    StyleValue::Str("::flats".to_string()),
                ),
            ]),
            HashMap::from([
                ("some-width".to_string(), StyleValue::Num(3.5)),
                (
                    "object-id".to_string(),
                    StyleValue::Str("::bridgeblack".to_string()),
                ),
            ]),
            HashMap::from([
                ("some-width".to_string(), StyleValue::Num(4.5)),
                (
                    "object-id".to_string(),
                    StyleValue::Str("::default".to_string()),
                ),
            ]),
        ];

        let mut sc = StyleChooser::new((15.0, 19.0));

        sc.new_object("");
        sc.add_condition(Condition::new(
            "eq",
            vec!["::class".to_string(), "::flats".to_string()],
        )); // `sc` styles apply to `::flats`
        sc.add_condition(parse_condition("oneway?").unwrap());

        sc.new_object("");
        sc.add_condition(Condition::new(
            "eq",
            vec!["::class".to_string(), "::bridgeblack".to_string()],
        )); // ... and to `::bridgeblack`
        sc.add_condition(parse_condition("oneway?").unwrap());

        sc.add_styles(vec![str_style(&[
            ("some-width", "1.5"),
            ("other-offset", "4"),
        ])]);

        let object_tags = tags(&[("highway", "service"), ("oneway", "yes")]);
        let mut cache = ChainMatchCache::new();
        sc.apply_styles(&mut styles, &object_tags, 1.0, 1.0, None, &mut cache);

        let expected_new_styles = vec![
            HashMap::from([
                // Selected by the first rule
                ("some-width".to_string(), StyleValue::Num(1.5)),
                ("other-offset".to_string(), StyleValue::Num(4.0)),
                (
                    "object-id".to_string(),
                    StyleValue::Str("::flats".to_string()),
                ),
            ]),
            HashMap::from([
                // Selected by the second rule
                ("some-width".to_string(), StyleValue::Num(1.5)),
                ("other-offset".to_string(), StyleValue::Num(4.0)),
                (
                    "object-id".to_string(),
                    StyleValue::Str("::bridgeblack".to_string()),
                ),
            ]),
            HashMap::from([
                // Style not changed (class is neither `::flats` nor `::bridgeblack`)
                ("some-width".to_string(), StyleValue::Num(4.5)),
                (
                    "object-id".to_string(),
                    StyleValue::Str("::default".to_string()),
                ),
            ]),
        ];

        assert_eq!(styles.len(), 3);
        assert_eq!(styles, expected_new_styles);
    }

    #[test]
    fn test_update_styles_by_class_all() {
        let mut styles = vec![
            HashMap::from([
                ("some-width".to_string(), StyleValue::Num(2.5)),
                ("corner-radius".to_string(), StyleValue::Num(2.5)),
                ("object-id".to_string(), StyleValue::Str("::*".to_string())),
            ]),
            HashMap::from([
                ("some-width".to_string(), StyleValue::Num(3.5)),
                (
                    "object-id".to_string(),
                    StyleValue::Str("::bridgeblack".to_string()),
                ),
            ]),
        ];

        let mut sc = StyleChooser::new((15.0, 19.0));

        sc.new_object("");
        sc.add_condition(parse_condition("tunnel").unwrap());

        sc.add_styles(vec![str_style(&[
            ("some-width", "1.5"),
            ("other-offset", "4"),
        ])]);
        let object_tags = tags(&[("highway", "service"), ("tunnel", "yes")]);
        let mut cache = ChainMatchCache::new();

        sc.apply_styles(&mut styles, &object_tags, 1.0, 1.0, None, &mut cache);

        let expected_new_style = HashMap::from([
            ("some-width".to_string(), StyleValue::Num(1.5)),
            ("corner-radius".to_string(), StyleValue::Num(2.5)),
            ("other-offset".to_string(), StyleValue::Num(4.0)),
            (
                "object-id".to_string(),
                StyleValue::Str("::default".to_string()),
            ),
        ]);

        assert_eq!(styles.len(), 3);
        assert_eq!(styles.last().unwrap(), &expected_new_style);
    }

    #[test]
    fn test_runtime_conditions() {
        // libkomwm builds one drule variant per reported condition set, so every
        // selected `::object-id` must report its own: the styles of an object-id
        // left out are dropped by the filter_by_runtime_conditions check.
        let mut sc = StyleChooser::new((4.0, 19.0));

        sc.new_object("");
        sc.add_condition(Condition::new(
            "eq",
            vec!["::class".to_string(), "::default".to_string()],
        ));
        sc.add_condition(parse_condition("place=city").unwrap());
        sc.add_runtime_condition(parse_condition("population>=1000").unwrap());

        sc.new_object("");
        sc.add_condition(Condition::new(
            "eq",
            vec!["::class".to_string(), "::int_name".to_string()],
        ));
        sc.add_condition(parse_condition("place=city").unwrap());
        sc.add_runtime_condition(parse_condition("population>=500").unwrap());

        let object_tags = tags(&[("place", "city")]);

        let mut cache = ChainMatchCache::new();
        assert_eq!(
            sc.get_runtime_conditions(&object_tags, &mut cache)
                .iter()
                .map(|rc| rc.iter().map(|c| c.repr()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            [
                vec!["population>=1000".to_string()],
                vec!["population>=500".to_string()]
            ]
        );

        // A chooser without runtime conditions reports none.
        let mut sc_plain = StyleChooser::new((4.0, 19.0));
        sc_plain.new_object("");
        sc_plain.add_condition(parse_condition("place=city").unwrap());
        let mut cache = ChainMatchCache::new();
        assert!(
            sc_plain
                .get_runtime_conditions(&object_tags, &mut cache)
                .is_empty()
        );
    }
}
