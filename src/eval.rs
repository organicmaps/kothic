//! MapCSS `eval(...)` expressions, ported from the Python reference.
//!
//! The Python implementation compiles the expression text as arbitrary Python and
//! evaluates it with a restricted set of built-ins (`tag`, `prop`, `num`, `metric`,
//! `zmetric`, `str`, `any`, `min`, `max`, `cond`, `boolean`). We parse the observed
//! grammar with a small recursive-descent parser instead.

use std::collections::{BTreeSet, HashMap};

use crate::compat;

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Str(String),
    None,
}

impl Val {
    pub fn py_str(&self) -> String {
        match self {
            Val::Num(n) => compat::float_str(*n),
            Val::Str(s) => s.clone(),
            Val::None => "None".to_string(),
        }
    }

    fn is_truthy(&self) -> bool {
        match self {
            Val::Num(n) => *n != 0.0,
            Val::Str(s) => !s.is_empty(),
            Val::None => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Str(String),
    Call(String, Vec<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

#[derive(Clone, Debug)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    LParen,
    RParen,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let mut toks = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let v = text
                .parse::<f64>()
                .map_err(|_| format!("bad number `{text}`"))?;
            toks.push(Tok::Num(v));
        } else if c == '\'' || c == '"' {
            let quote = c;
            i += 1;
            let mut out = String::new();
            loop {
                if i >= chars.len() {
                    return Err("unterminated string".to_string());
                }
                let ch = chars[i];
                if ch == quote {
                    i += 1;
                    break;
                } else if ch == '\\' {
                    i += 1;
                    if i >= chars.len() {
                        return Err("unterminated escape".to_string());
                    }
                    let e = chars[i];
                    out.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '\\' => '\\',
                        '\'' => '\'',
                        '"' => '"',
                        other => other,
                    });
                    i += 1;
                } else {
                    out.push(ch);
                    i += 1;
                }
            }
            toks.push(Tok::Str(out));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            toks.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            i += 1;
            toks.push(match c {
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' => Tok::Comma,
                '+' => Tok::Plus,
                '-' => Tok::Minus,
                '*' => Tok::Star,
                '/' => Tok::Slash,
                other => return Err(format!("unexpected char `{other}`")),
            });
        }
    }
    Ok(toks)
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_term()
    }

    fn parse_term(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_factor()?;
        while let Some(t) = self.peek() {
            let op = match t {
                Tok::Plus => '+',
                Tok::Minus => '-',
                _ => break,
            };
            self.next();
            let right = self.parse_factor()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        while let Some(t) = self.peek() {
            let op = match t {
                Tok::Star => '*',
                Tok::Slash => '/',
                _ => break,
            };
            self.next();
            let right = self.parse_unary()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if let Some(Tok::Minus) = self.peek() {
            self.next();
            return Ok(Expr::Neg(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Num(v)) => Ok(Expr::Num(v)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::LParen) => {
                let e = self.parse_expr()?;
                if !matches!(self.next(), Some(Tok::RParen)) {
                    return Err("expected `)`".to_string());
                }
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.next();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            args.push(self.parse_expr()?);
                            match self.next() {
                                Some(Tok::Comma) => continue,
                                Some(Tok::RParen) => break,
                                _ => return Err("expected `,` or `)`".to_string()),
                            }
                        }
                    } else {
                        self.next();
                    }
                    Ok(Expr::Call(name, args))
                } else {
                    Err(format!("bare identifier `{name}`"))
                }
            }
            _ => Err("expected expression".to_string()),
        }
    }
}

fn parse(s: &str) -> Result<Expr, String> {
    let toks = tokenize(s)?;
    let mut p = Parser {
        tokens: toks,
        pos: 0,
    };
    let e = p.parse_expr()?;
    if p.pos != p.tokens.len() {
        return Err("trailing tokens".to_string());
    }
    Ok(e)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Eval {
    expr_text: String,
    expr: Expr,
}

impl Eval {
    /// Parses a `eval( ... )` expression string.
    pub fn new(s: &str) -> Eval {
        let stripped = s.trim();
        let inner = stripped
            .strip_prefix("eval")
            .and_then(|r| r.trim_start().strip_prefix('('))
            .and_then(|r| r.strip_suffix(')'))
            .map(|r| r.trim())
            .unwrap_or(stripped);
        let expr_text = inner.to_string();
        let expr = parse(inner).unwrap_or(Expr::Num(0.0));
        Eval { expr_text, expr }
    }

    pub fn expr_text(&self) -> &str {
        &self.expr_text
    }

    pub fn repr(&self) -> String {
        format!("eval({})", self.expr_text)
    }

    pub fn extract_tags(&self) -> BTreeSet<String> {
        let mut tags = BTreeSet::new();
        collect_tags(&self.expr, &mut tags);
        tags
    }

    pub fn compute(
        &self,
        tags: &HashMap<String, String>,
        props: &HashMap<&str, Val>,
        xscale: f64,
        zscale: f64,
    ) -> String {
        match eval_expr(&self.expr, tags, props, xscale, zscale) {
            Ok(Val::Num(n)) => compat::g_format(n, 4),
            Ok(v) => v.py_str(),
            Err(_) => String::new(),
        }
    }
}

fn collect_tags(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::Call(name, args) => {
            if name == "tag" {
                for a in args {
                    if let Expr::Str(s) = a {
                        out.insert(s.clone());
                    }
                }
            }
            for a in args {
                collect_tags(a, out);
            }
        }
        Expr::Bin(_, a, b) => {
            collect_tags(a, out);
            collect_tags(b, out);
        }
        Expr::Neg(a) => collect_tags(a, out),
        _ => {}
    }
}

fn as_num(v: &Val) -> f64 {
    v.py_str().trim().parse::<f64>().unwrap_or(0.0)
}

fn as_metric(v: &Val, t: f64) -> Val {
    let x = v.py_str();
    if let Ok(n) = x.trim().parse::<f64>() {
        return Val::Num(n * t);
    }
    let x = x.trim();
    let ends = |s: &str| x.ends_with(s);
    let len = x.chars().count();
    if (ends("cm") || ends("CM") || ends("см")) && len >= 2 {
        let num: String = strip_chars(x, 2).trim().to_string();
        if let Ok(n) = num.parse::<f64>() {
            return Val::Num(n * t / 100.0);
        }
    }
    if (ends("mm") || ends("MM") || ends("мм")) && len >= 2 {
        let num: String = strip_chars(x, 2).trim().to_string();
        if let Ok(n) = num.parse::<f64>() {
            return Val::Num(n * t / 1000.0);
        }
    }
    if x.ends_with('m') || x.ends_with('M') || x.ends_with('м') {
        let num: String = strip_chars(x, 1).trim().to_string();
        if let Ok(n) = num.parse::<f64>() {
            return Val::Num(n * t);
        }
    }
    Val::None
}

/// Drops the last `n` characters of `x` on a char boundary (mirrors Python's
/// `x[:-n]`).
fn strip_chars(x: &str, n: usize) -> &str {
    let end = x.char_indices().nth(x.chars().count().saturating_sub(n));
    match end {
        Some((i, _)) => &x[..i],
        None => x,
    }
}

fn as_boolean(v: &Val) -> bool {
    let s = v.py_str();
    !(s.is_empty() || s == "0" || s == "no" || s == "false" || s == "False")
}

fn eval_expr(
    e: &Expr,
    tags: &HashMap<String, String>,
    props: &HashMap<&str, Val>,
    xscale: f64,
    zscale: f64,
) -> Result<Val, String> {
    match e {
        Expr::Num(n) => Ok(Val::Num(*n)),
        Expr::Str(s) => Ok(Val::Str(s.clone())),
        Expr::Neg(inner) => match eval_expr(inner, tags, props, xscale, zscale)? {
            Val::Num(n) => Ok(Val::Num(-n)),
            _ => Err("unary minus on non-number".to_string()),
        },
        Expr::Bin(op, a, b) => {
            let va = eval_expr(a, tags, props, xscale, zscale)?;
            let vb = eval_expr(b, tags, props, xscale, zscale)?;
            match (op, va, vb) {
                ('+', Val::Num(a), Val::Num(b)) => Ok(Val::Num(a + b)),
                ('-', Val::Num(a), Val::Num(b)) => Ok(Val::Num(a - b)),
                ('*', Val::Num(a), Val::Num(b)) => Ok(Val::Num(a * b)),
                ('/', Val::Num(_), Val::Num(0.0)) => Err("division by zero".to_string()),
                ('/', Val::Num(a), Val::Num(b)) => Ok(Val::Num(a / b)),
                ('+', Val::Str(a), Val::Str(b)) => Ok(Val::Str(a + &b)),
                _ => Err("operand type mismatch".to_string()),
            }
        }
        Expr::Call(name, args) => {
            let nargs = args.len();
            match name.as_str() {
                "tag" => {
                    if let Some(Expr::Str(key)) = args.first() {
                        Ok(Val::Str(tags.get(key).cloned().unwrap_or_default()))
                    } else {
                        Err("tag() needs a string key".to_string())
                    }
                }
                "prop" => {
                    if let Some(Expr::Str(key)) = args.first() {
                        Ok(props
                            .get(key.as_str())
                            .cloned()
                            .unwrap_or(Val::Str(String::new())))
                    } else {
                        Err("prop() needs a string key".to_string())
                    }
                }
                "num" => {
                    let mut v = Val::Num(0.0);
                    if let Some(a) = args.first() {
                        v = eval_expr(a, tags, props, xscale, zscale)?;
                    }
                    Ok(Val::Num(as_num(&v)))
                }
                "metric" => {
                    let mut v = Val::None;
                    if let Some(a) = args.first() {
                        v = eval_expr(a, tags, props, xscale, zscale)?;
                    }
                    Ok(as_metric(&v, xscale))
                }
                "zmetric" => {
                    let mut v = Val::None;
                    if let Some(a) = args.first() {
                        v = eval_expr(a, tags, props, xscale, zscale)?;
                    }
                    Ok(as_metric(&v, zscale))
                }
                "str" => {
                    let mut v = Val::None;
                    if let Some(a) = args.first() {
                        v = eval_expr(a, tags, props, xscale, zscale)?;
                    }
                    Ok(Val::Str(v.py_str()))
                }
                "any" => {
                    for a in args {
                        let v = eval_expr(a, tags, props, xscale, zscale)?;
                        if v.is_truthy() {
                            return Ok(v);
                        }
                    }
                    Ok(Val::Str(String::new()))
                }
                "min" => {
                    let mut vals = Vec::new();
                    for a in args {
                        let v = eval_expr(a, tags, props, xscale, zscale)?;
                        vals.push(as_num(&v));
                    }
                    let m = vals.into_iter().fold(f64::INFINITY, f64::min);
                    if m == f64::INFINITY {
                        Ok(Val::Num(0.0))
                    } else {
                        Ok(Val::Num(m))
                    }
                }
                "max" => {
                    let mut vals = Vec::new();
                    for a in args {
                        let v = eval_expr(a, tags, props, xscale, zscale)?;
                        vals.push(as_num(&v));
                    }
                    let m = vals.into_iter().fold(f64::NEG_INFINITY, f64::max);
                    if m == f64::NEG_INFINITY {
                        Ok(Val::Num(0.0))
                    } else {
                        Ok(Val::Num(m))
                    }
                }
                "cond" => {
                    if nargs == 3 {
                        let why = eval_expr(&args[0], tags, props, xscale, zscale)?;
                        let yes = eval_expr(&args[1], tags, props, xscale, zscale)?;
                        let no = eval_expr(&args[2], tags, props, xscale, zscale)?;
                        Ok(if as_boolean(&why) { yes } else { no })
                    } else {
                        Err("cond() needs 3 args".to_string())
                    }
                }
                "boolean" => {
                    let mut v = Val::None;
                    if let Some(a) = args.first() {
                        v = eval_expr(a, tags, props, xscale, zscale)?;
                    }
                    Ok(Val::Str(
                        if as_boolean(&v) { "True" } else { "False" }.to_string(),
                    ))
                }
                _ => Err(format!("unknown function `{name}`")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(map: &[(&str, &str)]) -> HashMap<String, String> {
        map.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn props<'a>(map: &[(&'a str, f64)]) -> HashMap<&'a str, Val> {
        map.iter().map(|(k, v)| (*k, Val::Num(*v))).collect()
    }

    #[test]
    fn test_tag() {
        let a = Eval::new("eval( tag(\"lanes\") )");
        assert_eq!(
            a.compute(&tags(&[("lanes", "4")]), &HashMap::new(), 1.0, 0.5),
            "4"
        );
        assert_eq!(
            a.compute(&tags(&[("natural", "trees")]), &HashMap::new(), 1.0, 0.5),
            ""
        );
        let mut t = BTreeSet::new();
        t.insert("lanes".to_string());
        assert_eq!(a.extract_tags(), t);
    }

    #[test]
    fn test_prop() {
        let a = Eval::new("eval( prop(\"dpi\") / 2 )");
        assert_eq!(
            a.compute(
                &tags(&[("lanes", "4")]),
                &props(&[("dpi", 144.0)]),
                1.0,
                0.5
            ),
            "72"
        );
        assert_eq!(
            a.compute(
                &tags(&[("lanes", "4")]),
                &props(&[("orientation", 0.0)]),
                1.0,
                0.5
            ),
            ""
        );
    }

    #[test]
    fn test_num() {
        let a = Eval::new("eval( num(tag(\"lanes\")) + 2 )");
        assert_eq!(
            a.compute(&tags(&[("lanes", "4")]), &HashMap::new(), 1.0, 0.5),
            "6"
        );
        assert_eq!(
            a.compute(&tags(&[("lanes", "many")]), &HashMap::new(), 1.0, 0.5),
            "2"
        );
    }

    #[test]
    fn test_metric() {
        let a = Eval::new("eval( metric(tag(\"height\")) )");
        for (h, exp) in [
            ("512", "512"),
            ("10m", "10"),
            (" 10m", "10"),
            ("500cm", "5"),
            ("500 cm", "5"),
            ("250CM", "2.5"),
            ("250 CM", "2.5"),
            ("30см", "0.3"),
            (" 30 см", "0.3"),
            ("1200 mm", "1.2"),
            ("2400MM", "2.4"),
            ("2800 мм", "2.8"),
        ] {
            assert_eq!(
                a.compute(&tags(&[("height", h)]), &HashMap::new(), 1.0, 0.5),
                exp,
                "height={h}"
            );
        }
    }

    #[test]
    fn test_metric_scale() {
        let a = Eval::new("eval( metric(tag(\"height\")) )");
        assert_eq!(
            a.compute(&tags(&[("height", "512")]), &HashMap::new(), 4.0, 4.0),
            "2048"
        );
        assert_eq!(
            a.compute(&tags(&[("height", "10m")]), &HashMap::new(), 4.0, 4.0),
            "40"
        );
        assert_eq!(
            a.compute(&tags(&[("height", "500cm")]), &HashMap::new(), 4.0, 4.0),
            "20"
        );
    }

    #[test]
    fn test_zmetric() {
        let a = Eval::new("eval( zmetric(tag(\"depth\")) )");
        assert_eq!(
            a.compute(&tags(&[("depth", "512")]), &HashMap::new(), 1.0, 0.5),
            "256"
        );
        assert_eq!(
            a.compute(&tags(&[("depth", "10m")]), &HashMap::new(), 1.0, 0.5),
            "5"
        );
        assert_eq!(
            a.compute(&tags(&[("depth", "30см")]), &HashMap::new(), 1.0, 0.5),
            "0.15"
        );
    }

    #[test]
    fn test_str_fn() {
        let a = Eval::new("eval( str( num(tag(\"width\")) - 200 ) )");
        assert_eq!(
            a.compute(&tags(&[("width", "400")]), &HashMap::new(), 1.0, 0.5),
            "200.0"
        );
    }

    #[test]
    fn test_any() {
        let a = Eval::new("eval( any(tag(\"building\"), tag(\"building:part\"), \"no\") )");
        assert_eq!(
            a.compute(
                &tags(&[("building", "apartment")]),
                &HashMap::new(),
                1.0,
                0.5
            ),
            "apartment"
        );
        assert_eq!(
            a.compute(
                &tags(&[("building:part", "roof")]),
                &HashMap::new(),
                1.0,
                0.5
            ),
            "roof"
        );
        assert_eq!(
            a.compute(
                &tags(&[("junction", "roundabout")]),
                &HashMap::new(),
                1.0,
                0.5
            ),
            "no"
        );
    }

    #[test]
    fn test_min_max() {
        let a = Eval::new("eval( min( num(tag(\"building:levels\")) * 3, 50) )");
        assert_eq!(
            a.compute(&tags(&[("natural", "wood")]), &HashMap::new(), 1.0, 0.5),
            "0"
        );
        assert_eq!(
            a.compute(
                &tags(&[("building:levels", "10")]),
                &HashMap::new(),
                1.0,
                0.5
            ),
            "30"
        );
        assert_eq!(
            a.compute(
                &tags(&[("building:levels", "30")]),
                &HashMap::new(),
                1.0,
                0.5
            ),
            "50"
        );

        let b = Eval::new("eval( max( tag(\"speed:limit\"), 60) )");
        assert_eq!(
            b.compute(&tags(&[("natural", "wood")]), &HashMap::new(), 1.0, 0.5),
            "60"
        );
        assert_eq!(
            b.compute(&tags(&[("speed:limit", "30")]), &HashMap::new(), 1.0, 0.5),
            "60"
        );
        assert_eq!(
            b.compute(&tags(&[("speed:limit", "90")]), &HashMap::new(), 1.0, 0.5),
            "90"
        );
    }

    #[test]
    fn test_cond() {
        let a = Eval::new("eval( cond( boolean(tag(\"oneway\")), 200, 100) )");
        assert_eq!(
            a.compute(&tags(&[("natural", "wood")]), &HashMap::new(), 1.0, 0.5),
            "100"
        );
        assert_eq!(
            a.compute(&tags(&[("oneway", "yes")]), &HashMap::new(), 1.0, 0.5),
            "200"
        );
        assert_eq!(
            a.compute(&tags(&[("oneway", "no")]), &HashMap::new(), 1.0, 0.5),
            "100"
        );
        assert_eq!(
            a.compute(&tags(&[("oneway", "true")]), &HashMap::new(), 1.0, 0.5),
            "200"
        );
    }

    #[test]
    fn test_complex() {
        let a = Eval::new(
            " eval( any( metric(tag(\"height\")), metric ( num(tag(\"building:levels\")) * 3), metric(\"1m\"))) ",
        );
        assert_eq!(
            a.compute(
                &tags(&[("building:levels", "3")]),
                &HashMap::new(),
                1.0,
                0.5
            ),
            "9"
        );
    }
}
