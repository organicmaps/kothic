//! MapCSS rule chains, ported from the Python reference implementation.

use std::collections::{BTreeSet, HashMap};

use crate::condition::{CondResult, Condition};

pub const TYPE_MATCHES: &[(&str, &[&str])] = &[
    ("", &["area", "line", "way", "node"]),
    ("area", &["area", "way"]),
    ("node", &["node"]),
    ("way", &["line", "area", "way"]),
    ("line", &["line", "area"]),
];

#[derive(Clone, Debug)]
pub struct Rule {
    pub runtime_conditions: Option<Vec<Condition>>,
    pub conditions: Vec<Condition>,
    pub min_zoom: f64,
    pub max_zoom: f64,
    pub subject: String,
    pub type_matches: Vec<String>,
}

impl Rule {
    pub fn new(s: &str) -> Rule {
        let subject = if s == "*" { "" } else { s };
        let type_matches = TYPE_MATCHES
            .iter()
            .find(|(k, _)| *k == subject)
            .map(|(_, v)| v.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        Rule {
            runtime_conditions: None,
            conditions: Vec::new(),
            min_zoom: 0.0,
            max_zoom: 19.0,
            subject: subject.to_string(),
            type_matches,
        }
    }

    pub fn repr(&self) -> String {
        format!(
            "{}|z{}-{} {:?} {:?}",
            self.subject, self.min_zoom, self.max_zoom, self.conditions, self.runtime_conditions
        )
    }

    pub fn test(&self, tags: &HashMap<String, String>) -> Option<String> {
        let mut subpart = "::default".to_string();
        for condition in &self.conditions {
            match condition.test(tags) {
                CondResult::Bool(false) => return None,
                CondResult::Bool(true) => {}
                CondResult::Subpart(s) => {
                    if s.is_empty() {
                        return None;
                    }
                    subpart = s;
                }
            }
        }
        Some(subpart)
    }

    pub fn get_compatible_types(&self) -> Vec<String> {
        TYPE_MATCHES
            .iter()
            .find(|(k, _)| *k == self.subject)
            .map(|(_, v)| v.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| vec![self.subject.clone()])
    }

    pub fn extract_tags(&self) -> BTreeSet<String> {
        let mut a = BTreeSet::new();
        for condition in &self.conditions {
            let tag = condition.extract_tag();
            if tag != "*" {
                a.insert(tag);
            } else if a.is_empty() {
                a.insert("*".to_string());
                return a;
            }
        }
        a
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

    #[test]
    fn test_subject() {
        let r = Rule::new("");
        assert_eq!(r.subject, "");
        assert_eq!(
            r.get_compatible_types(),
            vec!["area", "line", "way", "node"]
        );
        let r = Rule::new("*");
        assert_eq!(r.subject, "");
        let r = Rule::new("way");
        assert_eq!(r.get_compatible_types(), vec!["line", "area", "way"]);
        let r = Rule::new("line");
        assert_eq!(r.get_compatible_types(), vec!["line", "area"]);
        let r = Rule::new("area");
        assert_eq!(r.get_compatible_types(), vec!["area", "way"]);
        let r = Rule::new("node");
        assert_eq!(r.get_compatible_types(), vec!["node"]);
    }

    #[test]
    fn test_conditions() {
        let mut r = Rule::new("");
        r.conditions
            .push(parse_condition("highway=primary").unwrap());
        assert_eq!(
            r.test(&tags(&[("highway", "primary")])),
            Some("::default".to_string())
        );
        assert_eq!(r.test(&tags(&[("highway", "secondary")])), None);
    }

    #[test]
    fn test_class_subpart() {
        let mut r = Rule::new("");
        r.conditions.push(crate::condition::Condition::new(
            "eq",
            vec!["::class".to_string(), "::int_name".to_string()],
        ));
        assert_eq!(
            r.test(&tags(&[("any", "x")])),
            Some("::int_name".to_string())
        );
    }

    #[test]
    fn test_extract_tags() {
        let mut r = Rule::new("");
        r.conditions
            .push(parse_condition("highway=primary").unwrap());
        r.conditions.push(parse_condition("bridge?").unwrap());
        let mut expected = BTreeSet::new();
        expected.insert("highway".to_string());
        expected.insert("bridge".to_string());
        assert_eq!(r.extract_tags(), expected);
    }
}
