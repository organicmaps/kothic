//! MapCSS conditions: the condition classes and the condition-parsing half of
//! the original Python reference implementation.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CondResult {
    Bool(bool),
    Subpart(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub typ: String,
    pub params: Vec<String>,
    pub regex: Option<String>,
}

impl Condition {
    pub fn new(typ: &str, params: Vec<String>) -> Condition {
        let regex = if typ == "regex" {
            Some(params[1].clone())
        } else {
            None
        };
        Condition {
            typ: typ.to_string(),
            params,
            regex,
        }
    }

    pub fn extract_tag(&self) -> String {
        if self.params[0].starts_with("::") || self.typ == "regex" {
            "*".to_string()
        } else {
            self.params[0].clone()
        }
    }

    pub fn test(&self, tags: &HashMap<String, String>) -> CondResult {
        let t = self.typ.as_str();
        let p = &self.params;
        match t {
            "eq" => {
                if p[0].starts_with("::") {
                    CondResult::Subpart(p[1].clone())
                } else {
                    CondResult::Bool(tags.get(&p[0]).is_some() && tags.get(&p[0]) == Some(&p[1]))
                }
            }
            "ne" => CondResult::Bool(tags.get(&p[0]).is_none() || tags.get(&p[0]) != Some(&p[1])),
            "true" => CondResult::Bool(tags.get(&p[0]) == Some(&"yes".to_string())),
            "untrue" => CondResult::Bool(tags.get(&p[0]) == Some(&"no".to_string())),
            "set" => CondResult::Bool(
                tags.get(&p[0]).is_some() && tags.get(&p[0]) != Some(&"".to_string()),
            ),
            "unset" => CondResult::Bool(match tags.get(&p[0]) {
                Some(v) => v.is_empty(),
                None => true,
            }),
            _ => {
                if tags.get(&p[0]).is_none() {
                    return CondResult::Bool(false);
                }
                match t {
                    "regex" => {
                        let tag = &tags[&p[0]];
                        let re = self.regex.clone().unwrap_or_default();
                        let compiled = Regex::new(&format!("(?i)^{}", re)).unwrap();
                        CondResult::Bool(compiled.is_match(tag))
                    }
                    "<" => CondResult::Bool(number(&tags[&p[0]]) < number(&p[1])),
                    "<=" => CondResult::Bool(number(&tags[&p[0]]) <= number(&p[1])),
                    ">" => CondResult::Bool(number(&tags[&p[0]]) > number(&p[1])),
                    ">=" => CondResult::Bool(number(&tags[&p[0]]) >= number(&p[1])),
                    _ => CondResult::Bool(false),
                }
            }
        }
    }

    pub fn repr(&self) -> String {
        let t = self.typ.as_str();
        let p = &self.params;
        match t {
            "eq" if p[0].starts_with("::") => p[1].clone(),
            "eq" => format!("{}={}", p[0], p[1]),
            "ne" => format!("{}!={}", p[0], p[1]),
            "regex" => format!("{}=~/{}/", p[0], p[1]),
            "true" => format!("{}?", p[0]),
            "untrue" => format!("!{}?", p[0]),
            "set" => p[0].clone(),
            "unset" => format!("!{}", p[0]),
            "<" => format!("{}<{}", p[0], p[1]),
            "<=" => format!("{}<={}", p[0], p[1]),
            ">" => format!("{}>{}", p[0], p[1]),
            ">=" => format!("{}>={}", p[0], p[1]),
            _ => format!("{} {:?}", self.typ, self.params),
        }
    }
}

pub fn number(tt: &str) -> f64 {
    tt.trim().parse::<f64>().unwrap_or(0.0)
}

static RE_TRUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xi)\s*([:\w]+)\s*\?\s*$").unwrap());
static RE_INVTRUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xi)\s*!\s*([:\w]+)\s*\?\s*$").unwrap());
static RE_FALSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xi)\s*([:\w]+)\s*=\s*no\s*$").unwrap());
static RE_SET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(?x)\s*([-:\w]+)\s*$").unwrap());
static RE_UNSET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(?x)\s*!([:\w]+)\s*$").unwrap());
static RE_EQ: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xs)\s*([:\w]+)\s*=\s*(.+)\s*$").unwrap());
static RE_NE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xs)\s*([:\w]+)\s*!=\s*(.+)\s*$").unwrap());
static RE_GT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xs)\s*([:\w]+)\s*>\s*(.+)\s*$").unwrap());
static RE_GE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xs)\s*([:\w]+)\s*>=\s*(.+)\s*$").unwrap());
static RE_LT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xs)\s*([:\w]+)\s*<\s*(.+)\s*$").unwrap());
static RE_LE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xs)\s*([:\w]+)\s*<=\s*(.+)\s*$").unwrap());
static RE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?xs)\s*([:\w]+)\s*=~/\s*(.+)/\s*$").unwrap());

pub fn parse_condition(s: &str) -> Result<Condition, String> {
    if let Some(c) = RE_TRUE.captures(s) {
        let a = c.get(1).unwrap().as_str();
        return Ok(Condition::new("true", vec![a.to_string()]));
    }
    if let Some(c) = RE_INVTRUE.captures(s) {
        let a = c.get(1).unwrap().as_str();
        return Ok(Condition::new("ne", vec![a.to_string(), "yes".to_string()]));
    }
    if let Some(c) = RE_FALSE.captures(s) {
        let a = c.get(1).unwrap().as_str();
        return Ok(Condition::new("false", vec![a.to_string()]));
    }
    if let Some(c) = RE_SET.captures(s) {
        let a = c.get(1).unwrap().as_str();
        return Ok(Condition::new("set", vec![a.to_string()]));
    }
    if let Some(c) = RE_UNSET.captures(s) {
        let a = c.get(1).unwrap().as_str();
        return Ok(Condition::new("unset", vec![a.to_string()]));
    }
    if let Some(c) = RE_NE.captures(s) {
        let a = c.get(1).unwrap().as_str();
        let b = c.get(2).unwrap().as_str();
        return Ok(Condition::new("ne", vec![a.to_string(), b.to_string()]));
    }
    if let Some(c) = RE_LE.captures(s) {
        let a = c.get(1).unwrap().as_str();
        let b = c.get(2).unwrap().as_str();
        return Ok(Condition::new("<=", vec![a.to_string(), b.to_string()]));
    }
    if let Some(c) = RE_GE.captures(s) {
        let a = c.get(1).unwrap().as_str();
        let b = c.get(2).unwrap().as_str();
        return Ok(Condition::new(">=", vec![a.to_string(), b.to_string()]));
    }
    if let Some(c) = RE_LT.captures(s) {
        let a = c.get(1).unwrap().as_str();
        let b = c.get(2).unwrap().as_str();
        return Ok(Condition::new("<", vec![a.to_string(), b.to_string()]));
    }
    if let Some(c) = RE_GT.captures(s) {
        let a = c.get(1).unwrap().as_str();
        let b = c.get(2).unwrap().as_str();
        return Ok(Condition::new(">", vec![a.to_string(), b.to_string()]));
    }
    if let Some(c) = RE_REGEX.captures(s) {
        let a = c.get(1).unwrap().as_str();
        let b = c.get(2).unwrap().as_str();
        return Ok(Condition::new("regex", vec![a.to_string(), b.to_string()]));
    }
    if let Some(c) = RE_EQ.captures(s) {
        let a = c.get(1).unwrap().as_str();
        let b = c.get(2).unwrap().as_str();
        return Ok(Condition::new("eq", vec![a.to_string(), b.to_string()]));
    }
    Err(format!("condition UNKNOWN: {}", s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(map: &[(&str, &str)]) -> HashMap<String, String> {
        map.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_parse_eq() {
        let c = parse_condition("highway=primary").unwrap();
        assert_eq!(c.typ, "eq");
        assert_eq!(c.params, vec!["highway", "primary"]);
        assert_eq!(c.extract_tag(), "highway");
        assert_eq!(
            c.test(&tags(&[("highway", "primary")])),
            CondResult::Bool(true)
        );
        assert_eq!(
            c.test(&tags(&[("highway", "secondary")])),
            CondResult::Bool(false)
        );
        assert_eq!(c.test(&tags(&[])), CondResult::Bool(false));
    }

    #[test]
    fn test_parse_regex() {
        let c = parse_condition("ref=~/^M\\d+$/").unwrap();
        assert_eq!(c.typ, "regex");
        assert_eq!(c.extract_tag(), "*");
        assert_eq!(c.test(&tags(&[("ref", "M5")])), CondResult::Bool(true));
        assert_eq!(c.test(&tags(&[("ref", "xM5")])), CondResult::Bool(false));
        assert_eq!(c.test(&tags(&[])), CondResult::Bool(false));
    }

    #[test]
    fn test_parse_ge_gt() {
        let c = parse_condition("population>=1000").unwrap();
        assert_eq!(c.typ, ">=");
        assert_eq!(
            c.test(&tags(&[("population", "1000")])),
            CondResult::Bool(true)
        );
        assert_eq!(
            c.test(&tags(&[("population", "999")])),
            CondResult::Bool(false)
        );
        let c = parse_condition("population>1000").unwrap();
        assert_eq!(c.typ, ">");
        assert_eq!(
            c.test(&tags(&[("population", "1000")])),
            CondResult::Bool(false)
        );
        assert_eq!(
            c.test(&tags(&[("population", "1001")])),
            CondResult::Bool(true)
        );
    }

    #[test]
    fn test_parse_lt_le() {
        let c = parse_condition("population<1000").unwrap();
        assert_eq!(c.typ, "<");
        assert_eq!(
            c.test(&tags(&[("population", "999")])),
            CondResult::Bool(true)
        );
        let c = parse_condition("population<=1000").unwrap();
        assert_eq!(c.typ, "<=");
        assert_eq!(
            c.test(&tags(&[("population", "1000")])),
            CondResult::Bool(true)
        );
    }

    #[test]
    fn test_parse_ne() {
        let c = parse_condition("highway!=primary").unwrap();
        assert_eq!(c.typ, "ne");
        assert_eq!(
            c.test(&tags(&[("highway", "primary")])),
            CondResult::Bool(false)
        );
        assert_eq!(
            c.test(&tags(&[("highway", "secondary")])),
            CondResult::Bool(true)
        );
        assert_eq!(c.test(&tags(&[])), CondResult::Bool(true));
    }

    #[test]
    fn test_parse_set_unset() {
        let c = parse_condition("name").unwrap();
        assert_eq!(c.typ, "set");
        assert_eq!(c.test(&tags(&[("name", "foo")])), CondResult::Bool(true));
        assert_eq!(c.test(&tags(&[("name", "")])), CondResult::Bool(false));
        assert_eq!(c.test(&tags(&[])), CondResult::Bool(false));

        let c = parse_condition("!name").unwrap();
        assert_eq!(c.typ, "unset");
        assert_eq!(c.test(&tags(&[])), CondResult::Bool(true));
        assert_eq!(c.test(&tags(&[("name", "")])), CondResult::Bool(true));
        assert_eq!(c.test(&tags(&[("name", "foo")])), CondResult::Bool(false));
    }

    #[test]
    fn test_parse_true_false() {
        let c = parse_condition("bridge?").unwrap();
        assert_eq!(c.typ, "true");
        assert_eq!(c.test(&tags(&[("bridge", "yes")])), CondResult::Bool(true));
        assert_eq!(c.test(&tags(&[("bridge", "no")])), CondResult::Bool(false));

        let c = parse_condition("bridge=no").unwrap();
        assert_eq!(c.typ, "false");
        // Python `false` conditions always test `False` (not implemented).
        assert_eq!(c.test(&tags(&[("bridge", "no")])), CondResult::Bool(false));

        let c = parse_condition("!bridge?").unwrap();
        assert_eq!(c.typ, "ne");
        assert_eq!(c.test(&tags(&[("bridge", "yes")])), CondResult::Bool(false));
    }

    #[test]
    fn test_subpart() {
        // A bare `::`-token parses as a plain `set` condition (matches Python).
        let c = parse_condition("::sublayer").unwrap();
        assert_eq!(c.typ, "set");
        assert_eq!(c.extract_tag(), "*");
        assert_eq!(c.test(&tags(&[])), CondResult::Bool(false));
    }

    #[test]
    fn test_repr() {
        assert_eq!(
            parse_condition("highway=primary").unwrap().repr(),
            "highway=primary"
        );
        assert_eq!(parse_condition("name").unwrap().repr(), "name");
        assert_eq!(parse_condition("!name").unwrap().repr(), "!name");
        assert_eq!(parse_condition("bridge?").unwrap().repr(), "bridge?");
        assert_eq!(
            parse_condition("population>=1000").unwrap().repr(),
            "population>=1000"
        );
        assert_eq!(
            parse_condition("highway!=primary").unwrap().repr(),
            "highway!=primary"
        );
        assert_eq!(
            parse_condition("ref=~/^M\\d+$/").unwrap().repr(),
            "ref=~/^M\\d+$/"
        );
    }
}
