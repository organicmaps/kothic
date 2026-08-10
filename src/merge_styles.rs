//! Merges several single-variant native drules containers by taking the union
//! of each type's zoom range (a missing low/high zoom in one style is filled
//! from another), ported from the Python `merge_styles` module.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::drules::{ClassifElement, Container, DrawElement};

/// For each type: `(lowest-zoom element, highest-zoom element, all elements)`.
pub type ZoomExtremes = IndexMap<String, (DrawElement, DrawElement, Vec<DrawElement>)>;

pub fn read_zoom_extremes(cont: &Container) -> ZoomExtremes {
    let mut result = ZoomExtremes::new();
    for rule in &cont.cont {
        let mut low: Option<DrawElement> = None;
        let mut high: Option<DrawElement> = None;
        let all = rule.element.clone();
        for elem in &rule.element {
            if high.as_ref().is_none_or(|h| elem.scale > h.scale) {
                high = Some(elem.clone());
            }
            if low.as_ref().is_none_or(|l| elem.scale < l.scale) {
                low = Some(elem.clone());
            }
        }
        if let (Some(low_e), Some(high_e)) = (low, high) {
            match result.get_mut(&rule.name) {
                Some(entry) => {
                    if low_e.scale < entry.0.scale {
                        entry.0 = low_e;
                    }
                    if high_e.scale > entry.1.scale {
                        entry.1 = high_e;
                    }
                    entry.2.extend(all);
                }
                None => {
                    result.insert(rule.name.clone(), (low_e, high_e, all));
                }
            }
        }
    }
    result
}

pub fn zooms_string(z1: u8, z2: u8) -> String {
    if z2 != z1 {
        format!("zooms {}-{}", z1.min(z2), z1.max(z2))
    } else {
        format!("zoom {}", z1)
    }
}

/// Appends as many copies of `source` (re-scaled) to `dest[typ]` as needed to
/// extend `target`'s zoom range to cover `source`'s.
pub fn add_missing_zooms(
    dest: &mut HashMap<String, Vec<DrawElement>>,
    typ: &str,
    source: &DrawElement,
    target: &DrawElement,
    high: bool,
) {
    let (a, b) = if high {
        (target.scale as i64 + 1, source.scale as i64 + 1)
    } else {
        (source.scale as i64, target.scale as i64)
    };

    if b < a {
        println!(
            "{}: missing {} {}",
            typ,
            if high { "high" } else { "low" },
            zooms_string(b as u8, (a - 1) as u8)
        );
        for z in b..a {
            let mut fix = source.clone();
            fix.scale = z as u8;
            dest.entry(typ.to_string()).or_default().push(fix);
        }
    } else if b > a {
        println!(
            "{}: extra {} {}",
            typ,
            if high { "high" } else { "low" },
            zooms_string(a as u8, (b - 1) as u8)
        );
    }
}

pub type Diff = (
    HashMap<String, Vec<DrawElement>>,
    HashMap<String, Vec<DrawElement>>,
    Vec<ClassifElement>,
);

pub fn create_diff(zooms1: &ZoomExtremes, zooms2: &ZoomExtremes) -> Diff {
    let mut add_elements_low: HashMap<String, Vec<DrawElement>> = HashMap::new();
    let mut add_elements_high: HashMap<String, Vec<DrawElement>> = HashMap::new();
    let mut seen: HashSet<String> = zooms2.keys().cloned().collect();
    for (typ, e1) in zooms1 {
        if let Some(e2) = zooms2.get(typ) {
            seen.remove(typ);
            add_missing_zooms(&mut add_elements_low, typ, &e1.0, &e2.0, false);
            add_missing_zooms(&mut add_elements_high, typ, &e1.1, &e2.1, true);
        } else {
            println!(
                "{}: not found in the alternative style; {}",
                typ,
                zooms_string(e1.0.scale, e1.1.scale)
            );
        }
    }

    let mut add_types: Vec<ClassifElement> = Vec::new();
    let mut missing: Vec<String> = seen.into_iter().collect();
    missing.sort();
    for typ in missing {
        let e2 = &zooms2[&typ];
        println!(
            "{}: missing completely; {}",
            typ,
            zooms_string(e2.0.scale, e2.1.scale)
        );
        let mut cont = ClassifElement::new();
        cont.set_name(typ.as_str());
        cont.element.extend(e2.2.clone());
        add_types.push(cont);
    }

    (add_elements_low, add_elements_high, add_types)
}

pub fn apply_diff(cont: &Container, diff: &Diff) -> Container {
    let mut d2pos = 0;
    let mut result = Container::new();
    result.colors.value.extend(cont.colors.value.clone());
    for rule in &cont.cont {
        let typ = &rule.name;
        // Append diff types whose name sorts before typ.
        while d2pos < diff.2.len() && diff.2[d2pos].name < *typ {
            result.cont.push(diff.2[d2pos].clone());
            d2pos += 1;
        }
        let mut fix = ClassifElement::new();
        fix.set_name(typ.as_str());
        if let Some(list) = diff.0.get(typ) {
            fix.element.extend(list.clone());
        }
        if !rule.element.is_empty() {
            fix.element.extend(rule.element.clone());
        }
        if let Some(list) = diff.1.get(typ) {
            fix.element.extend(list.clone());
        }
        result.cont.push(fix);
    }
    for extra in diff.2.iter().skip(d2pos) {
        result.cont.push(extra.clone());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drules::{ClassifElement, ColorElement, DrawElement, LineRule};

    fn make_element(scale: u8, width: f64) -> DrawElement {
        let mut element = DrawElement::new();
        element.set_scale(scale);
        let mut line = LineRule::new();
        line.set_width(width);
        line.set_color(0xFF010203u32);
        element.lines.push(line);
        element
    }

    fn make_container(types: &[(&str, Vec<DrawElement>)]) -> Container {
        let mut container = Container::new();
        let mut color = ColorElement::new();
        color.set_name("base");
        color.set_color(0xFF010203u32);
        container.colors.value.push(color);
        for (name, elements) in types {
            let mut rule = ClassifElement::new();
            rule.set_name(*name);
            rule.element.extend(elements.clone());
            container.cont.push(rule);
        }
        container
    }

    #[test]
    fn test_merge_styles_preserves_missing_type_elements_and_colors() {
        let base = make_container(&[("type-b", vec![make_element(10, 1.0)])]);
        let alternative =
            make_container(&[("type-a", vec![make_element(5, 2.0), make_element(7, 4.0)])]);

        let diff = create_diff(
            &read_zoom_extremes(&base),
            &read_zoom_extremes(&alternative),
        );
        let merged = apply_diff(&base, &diff);

        assert_eq!(merged.colors.value.len(), 1);
        let names: Vec<&str> = merged.cont.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["type-a", "type-b"]);
        let scales: Vec<u8> = merged.cont[0].element.iter().map(|e| e.scale).collect();
        assert_eq!(scales, vec![5, 7]);
        let widths: Vec<f64> = merged.cont[0]
            .element
            .iter()
            .map(|e| e.lines[0].width)
            .collect();
        assert_eq!(widths, vec![2.0, 4.0]);
    }

    #[test]
    fn test_add_missing_high_zooms() {
        let mut base =
            make_container(&[("type-b", vec![make_element(5, 1.0), make_element(10, 2.0)])]);
        let alternative = make_container(&[("type-b", vec![make_element(12, 3.0)])]);
        let diff = create_diff(
            &read_zoom_extremes(&base),
            &read_zoom_extremes(&alternative),
        );
        base = apply_diff(&base, &diff);
        let scales: Vec<u8> = base.cont[0].element.iter().map(|e| e.scale).collect();
        // High-zoom fill: target range up to 10, source 12 -> adds 11 and 12.
        assert_eq!(scales, vec![5, 10, 11, 12]);
    }
}
