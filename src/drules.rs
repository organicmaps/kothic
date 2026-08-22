//! Native drawing-rules format, ported from the Python `drules` module.
//!
//! Provides the builder classes (`Container`, `ClassifElement`, `DrawElement`, ...)
//! mirroring what the pipeline expects (attribute assignment, eager sub-rules,
//! repeated `.extend()`/`.push()`, canonical `str()` for de-duplication), plus a
//! binary writer/reader for the native format and a canonical text dumper.
//!
//! The wire format MUST stay in sync with `libs/indexer/drules_format.{hpp,cpp}`.
//!
//! Color model: builder objects hold actual ARGB color VALUES; the palette+index
//! encoding is an internal detail of the binary writer (values -> indices) and
//! reader (indices -> values).

use std::collections::HashMap;
use std::path::Path;

use crate::compat::{float_str, g_format, repr_str};

// --- Wire constants (keep in sync with libs/indexer/drules_format.hpp) ---
pub const MAGIC: &[u8] = b"OMDR";
pub const VERSION: u8 = 1;

pub const LINE_FLAG_DASHDOT: u8 = 1 << 0;
pub const LINE_FLAG_PATHSYM: u8 = 1 << 1;
pub const AREA_FLAG_BORDER: u8 = 1 << 0;
pub const CAPTION_FLAG_PRIMARY: u8 = 1 << 0;
pub const CAPTION_FLAG_SECONDARY: u8 = 1 << 1;

pub const KIND_LINES: u8 = 1 << 0;
pub const KIND_AREA: u8 = 1 << 1;
pub const KIND_SYMBOL: u8 = 1 << 2;
pub const KIND_CAPTION: u8 = 1 << 3;
pub const KIND_PATHTEXT: u8 = 1 << 4;
pub const KIND_SHIELD: u8 = 1 << 5;

/// LineCap / LineJoin enum values (match `drules_struct.hpp`).
pub const ROUND_CAP: u8 = 0;
pub const BUTT_CAP: u8 = 1;
pub const SQUARE_CAP: u8 = 2;
pub const ROUND_JOIN: u8 = 0;
pub const BEVEL_JOIN: u8 = 1;
pub const NO_JOIN: u8 = 2;

// The per-variant color scalars (everything else is shared across a family's variants).
const COLOR_FIELDS: &[&str] = &["color", "stroke_color", "text_color", "text_stroke_color"];

/// Reference-compatible `repr()` of scalar values, used by `canonical()`.
trait ScalarRepr {
    fn repr(&self) -> String;
}

impl ScalarRepr for f64 {
    fn repr(&self) -> String {
        float_str(*self)
    }
}

impl ScalarRepr for u8 {
    fn repr(&self) -> String {
        self.to_string()
    }
}

impl ScalarRepr for u32 {
    fn repr(&self) -> String {
        self.to_string()
    }
}

impl ScalarRepr for i32 {
    fn repr(&self) -> String {
        self.to_string()
    }
}

impl ScalarRepr for bool {
    fn repr(&self) -> String {
        if *self {
            "True".to_string()
        } else {
            "False".to_string()
        }
    }
}

impl ScalarRepr for String {
    fn repr(&self) -> String {
        repr_str(self)
    }
}

fn scalar_repr<T: ScalarRepr>(v: &T) -> String {
    v.repr()
}

fn list_repr<T: ScalarRepr>(v: &[T]) -> String {
    let inner: Vec<String> = v.iter().map(|x| scalar_repr(x)).collect();
    format!("[{}]", inner.join(", "))
}

/// Defines a builder message.
macro_rules! msg {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(scalars: { $($sname:ident: $sty:ty => $sdefault:expr),* $(,)? })*
            $(repeated_scalar: [ $($rname:ident: $rty:ty),* $(,)? ])*
            $(submsgs: { $($subname:ident: $subty:ty),* $(,)? })*
            $(repeated_msg: [ $($rmname:ident: $rmty:ty),* $(,)? ])*
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone)]
        pub struct $name {
            pub present: bool,
            $( $( pub $sname: $sty, )* )*
            $( $( pub $rname: Vec<$rty>, )* )*
            $( $( pub $subname: $subty, )* )*
            $( $( pub $rmname: Vec<$rmty>, )* )*
        }

        impl $name {
            pub fn new() -> Self {
                $name {
                    present: false,
                    $( $( $sname: $sdefault, )* )*
                    $( $( $rname: Vec::new(), )* )*
                    $( $( $subname: <$subty>::new(), )* )*
                    $( $( $rmname: Vec::new(), )* )*
                }
            }

            $( $( paste::paste! {
                pub fn [<set_ $sname>]<V: Into<$sty>>(&mut self, v: V) {
                    self.$sname = v.into();
                    self.present = true;
                }
            } )* )*
            /// A message is "set" once any scalar was assigned, a repeated field
            /// is non-empty, or one of its sub-messages is set.
            pub fn is_set(&self) -> bool {
                self.present
                    $( $( || !self.$rname.is_empty() )* )*
                    $( $( || self.$subname.is_set() )* )*
                    $( $( || !self.$rmname.is_empty() )* )*
            }

            /// Deterministic content string used as a de-duplication key. With
            /// `skip_colors` the four color fields are omitted, giving a
            /// structure-only key for the variant isomorphism check.
            pub fn canonical(&self, skip_colors: bool) -> String {
                let mut parts: Vec<String> = Vec::new();
                $(
                    $(
                        let name = stringify!($sname);
                        if !(skip_colors && COLOR_FIELDS.contains(&name)) {
                            let v = &self.$sname;
                            if v != &$sdefault {
                                parts.push(format!("{}={}", name, scalar_repr(v)));
                            }
                        }
                    )*
                )*
                $(
                    $(
                        if !self.$rname.is_empty() {
                            parts.push(format!("{}={}", stringify!($rname), list_repr(&self.$rname)));
                        }
                    )*
                )*
                $(
                    $(
                        if self.$subname.is_set() {
                            parts.push(format!(
                                "{}{{{}}}",
                                stringify!($subname),
                                self.$subname.canonical(skip_colors)
                            ));
                        }
                    )*
                )*
                $(
                    $(
                        if !self.$rmname.is_empty() {
                            let inner: Vec<String> = self
                                .$rmname
                                .iter()
                                .map(|m| m.canonical(skip_colors))
                                .collect();
                            parts.push(format!("{}=[{}]", stringify!($rmname), inner.join(",")));
                        }
                    )*
                )*
                parts.join(",")
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.canonical(false))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

msg! {
    /// Line dash-dot pattern.
    DashDot {
        scalars: { offset: f64 => 0.0 }
        repeated_scalar: [ dd: f64 ]
    }
}

msg! {
    /// Repeated path symbol drawn along a line.
    PathSym {
        scalars: { name: String => String::new(), step: f64 => 0.0, offset: f64 => 0.0 }
    }
}

msg! {
    /// A line as used for area borders (no priority on the wire).
    LineDef {
        scalars: { width: f64 => 0.0, color: u32 => 0, join: u8 => ROUND_JOIN, cap: u8 => ROUND_CAP }
        submsgs: { dashdot: DashDot, pathsym: PathSym }
    }
}

msg! {
    LineRule {
        scalars: { width: f64 => 0.0, color: u32 => 0, priority: i32 => 0, join: u8 => ROUND_JOIN, cap: u8 => ROUND_CAP }
        submsgs: { dashdot: DashDot, pathsym: PathSym }
    }
}

/// Shared shape of `LineRule` and `LineDef` (the latter is a line without a
/// priority on the wire, used for area borders).
trait LineLike {
    fn width(&self) -> f64;
    fn color(&self) -> u32;
    fn priority(&self) -> i32;
    fn join(&self) -> u8;
    fn cap(&self) -> u8;
    fn dashdot(&self) -> &DashDot;
    fn pathsym(&self) -> &PathSym;
}

impl LineLike for LineRule {
    fn width(&self) -> f64 {
        self.width
    }
    fn color(&self) -> u32 {
        self.color
    }
    fn priority(&self) -> i32 {
        self.priority
    }
    fn join(&self) -> u8 {
        self.join
    }
    fn cap(&self) -> u8 {
        self.cap
    }
    fn dashdot(&self) -> &DashDot {
        &self.dashdot
    }
    fn pathsym(&self) -> &PathSym {
        &self.pathsym
    }
}

impl LineLike for LineDef {
    fn width(&self) -> f64 {
        self.width
    }
    fn color(&self) -> u32 {
        self.color
    }
    fn priority(&self) -> i32 {
        0
    }
    fn join(&self) -> u8 {
        self.join
    }
    fn cap(&self) -> u8 {
        self.cap
    }
    fn dashdot(&self) -> &DashDot {
        &self.dashdot
    }
    fn pathsym(&self) -> &PathSym {
        &self.pathsym
    }
}

trait LineLikeMut {
    fn set_color_idx(&mut self, idx: u32);
}

impl LineLikeMut for LineRule {
    fn set_color_idx(&mut self, idx: u32) {
        self.set_color(idx);
    }
}

impl LineLikeMut for LineDef {
    fn set_color_idx(&mut self, idx: u32) {
        self.set_color(idx);
    }
}

msg! {
    AreaRule {
        scalars: { color: u32 => 0, priority: i32 => 0 }
        submsgs: { border: LineDef }
    }
}

msg! {
    SymbolRule {
        scalars: { name: String => String::new(), apply_for_type: i32 => 0, priority: i32 => 0, min_distance: i32 => 0 }
    }
}

msg! {
    CaptionDef {
        scalars: { height: i32 => 0, color: u32 => 0, stroke_color: u32 => 0, offset_x: i32 => 0, offset_y: i32 => 0, text: String => String::new(), is_optional: bool => false }
    }
}

msg! {
    CaptionRule {
        scalars: { priority: i32 => 0 }
        submsgs: { primary: CaptionDef, secondary: CaptionDef }
    }
}

/// PathText has the exact same shape as Caption.
pub type PathTextRule = CaptionRule;

msg! {
    ShieldRule {
        scalars: { height: i32 => 0, color: u32 => 0, stroke_color: u32 => 0, priority: i32 => 0, min_distance: i32 => 0, text_color: u32 => 0, text_stroke_color: u32 => 0 }
    }
}

msg! {
    DrawElement {
        scalars: { scale: u8 => 0 }
        repeated_scalar: [ apply_if: String ]
        submsgs: { area: AreaRule, symbol: SymbolRule, caption: CaptionRule, path_text: PathTextRule, shield: ShieldRule }
        repeated_msg: [ lines: LineRule ]
    }
}

msg! {
    ClassifElement {
        scalars: { name: String => String::new() }
        repeated_msg: [ element: DrawElement ]
    }
}

msg! {
    ColorElement {
        scalars: { name: String => String::new(), color: u32 => 0 }
    }
}

msg! {
    ColorsElement {
        repeated_msg: [ value: ColorElement ]
    }
}

msg! {
    Container {
        submsgs: { colors: ColorsElement }
        repeated_msg: [ cont: ClassifElement ]
    }
}

fn write_varuint(out: &mut Vec<u8>, mut value: u64) {
    while value > 127 {
        out.push(((value & 127) | 128) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// ZigZag of a signed int32 (the C++ reader reads these via
/// `ReadVarInt<int32_t>`), matching `coding/varint.hpp` WriteVarInt +
/// `bits::ZigZagEncode`.
fn write_varint(out: &mut Vec<u8>, value: i64) {
    debug_assert!((-(1i64 << 31)..(1i64 << 31)).contains(&value), "{}", value);
    let u = ((value << 1) ^ (value >> 63)) as u64;
    write_varuint(out, u);
}

fn write_f32(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&(value as f32).to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let data = s.as_bytes();
    write_varuint(out, data.len() as u64);
    out.extend_from_slice(data);
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8, String> {
        let b = *self.data.get(self.pos).ok_or("Truncated drules data")?;
        self.pos += 1;
        Ok(b)
    }

    fn varuint(&mut self) -> Result<u64, String> {
        let mut shift: u32 = 0;
        let mut result: u64 = 0;
        loop {
            let b = self.u8()?;
            result |= ((b & 127) as u64) << shift;
            if b & 128 == 0 {
                return Ok(result);
            }
            shift += 7;
        }
    }

    fn varint(&mut self) -> Result<i64, String> {
        let u = self.varuint()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    fn f32(&mut self) -> Result<f64, String> {
        let s = self.raw(4)?;
        let mut b = [0u8; 4];
        b.copy_from_slice(s);
        Ok(f32::from_le_bytes(b) as f64)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let s = self.raw(4)?;
        let mut b = [0u8; 4];
        b.copy_from_slice(s);
        Ok(u32::from_le_bytes(b))
    }

    fn string(&mut self) -> Result<String, String> {
        let n = self.varuint()? as usize;
        let s = self.raw(n)?;
        Ok(String::from_utf8_lossy(s).into_owned())
    }

    fn raw(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.pos + n > self.data.len() {
            return Err("Truncated drules data".into());
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

/// Result of `build_indexed`: (template, palette, named) where `template` is a
/// copy of `containers[0]` with colors replaced by palette indices.
type IndexedResult = Result<(Container, Vec<Vec<u32>>, Vec<(String, usize)>), String>;

/// Walks the variant containers in lockstep, returning `(template, palette, named)`:
///   * template: a copy of containers[0] with every color scalar replaced by its palette index;
///   * palette : list of per-variant color tuples (`palette[idx][variant]` is the color value);
///   * named   : list of (name, palette_index) for the colors{} block.
///
/// Errors unless all variants are isomorphic modulo the four color fields.
fn build_indexed(containers: &[Container]) -> IndexedResult {
    let base = containers[0].canonical(true);
    for c in &containers[1..] {
        if c.canonical(true) != base {
            return Err(
                "drules variants are not isomorphic modulo colors; per-variant overrides are not supported yet"
                    .to_string(),
            );
        }
    }

    let mut template = containers[0].clone();
    let mut palette = Palette::new();

    let mut named: Vec<(String, usize)> = Vec::new();
    for m in 0..template.colors.value.len() {
        let key: Vec<u32> = containers.iter().map(|c| c.colors.value[m].color).collect();
        named.push((template.colors.value[m].name.clone(), palette.cidx(key)));
    }

    for i in 0..template.cont.len() {
        let ocs: Vec<&ClassifElement> = containers.iter().map(|c| &c.cont[i]).collect();
        for j in 0..template.cont[i].element.len() {
            let oe: Vec<&DrawElement> = ocs.iter().map(|oc| &oc.element[j]).collect();
            index_element(&mut template.cont[i].element[j], &oe, &mut palette);
        }
    }

    Ok((template, palette.entries, named))
}

struct Palette {
    entries: Vec<Vec<u32>>,
    index: HashMap<Vec<u32>, usize>,
}

impl Palette {
    fn new() -> Self {
        Palette {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    fn cidx(&mut self, key: Vec<u32>) -> usize {
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        let i = self.entries.len();
        self.index.insert(key.clone(), i);
        self.entries.push(key);
        i
    }
}

fn index_linedef<T: LineLike + LineLikeMut>(t: &mut T, objs: &[&T], palette: &mut Palette) {
    let key: Vec<u32> = objs.iter().map(|o| o.color()).collect();
    t.set_color_idx(palette.cidx(key) as u32);
}

fn index_captiondef(t: &mut CaptionDef, objs: &[&CaptionDef], palette: &mut Palette) {
    let key: Vec<u32> = objs.iter().map(|o| o.color).collect();
    t.set_color(palette.cidx(key) as u32);
    let key: Vec<u32> = objs.iter().map(|o| o.stroke_color).collect();
    t.set_stroke_color(palette.cidx(key) as u32);
}

fn index_caption(t: &mut CaptionRule, objs: &[&CaptionRule], palette: &mut Palette) {
    if t.primary.is_set() {
        let odefs: Vec<&CaptionDef> = objs.iter().map(|o| &o.primary).collect();
        index_captiondef(&mut t.primary, &odefs, palette);
    }
    if t.secondary.is_set() {
        let odefs: Vec<&CaptionDef> = objs.iter().map(|o| &o.secondary).collect();
        index_captiondef(&mut t.secondary, &odefs, palette);
    }
}

fn index_element(t: &mut DrawElement, objs: &[&DrawElement], palette: &mut Palette) {
    for k in 0..t.lines.len() {
        let ol: Vec<&LineRule> = objs.iter().map(|o| &o.lines[k]).collect();
        index_linedef(&mut t.lines[k], &ol, palette);
    }
    if t.area.is_set() {
        let oa: Vec<&AreaRule> = objs.iter().map(|o| &o.area).collect();
        let key: Vec<u32> = oa.iter().map(|a| a.color).collect();
        t.area.set_color(palette.cidx(key) as u32);
        if t.area.border.is_set() {
            let ob: Vec<&LineDef> = oa.iter().map(|a| &a.border).collect();
            index_linedef(&mut t.area.border, &ob, palette);
        }
    }
    if t.caption.is_set() {
        let oc: Vec<&CaptionRule> = objs.iter().map(|o| &o.caption).collect();
        index_caption(&mut t.caption, &oc, palette);
    }
    if t.path_text.is_set() {
        let op: Vec<&PathTextRule> = objs.iter().map(|o| &o.path_text).collect();
        index_caption(&mut t.path_text, &op, palette);
    }
    if t.shield.is_set() {
        let osh: Vec<&ShieldRule> = objs.iter().map(|o| &o.shield).collect();
        let key: Vec<u32> = osh.iter().map(|o| o.color).collect();
        t.shield.set_color(palette.cidx(key) as u32);
        let key: Vec<u32> = osh.iter().map(|o| o.stroke_color).collect();
        t.shield.set_stroke_color(palette.cidx(key) as u32);
        let key: Vec<u32> = osh.iter().map(|o| o.text_color).collect();
        t.shield.set_text_color(palette.cidx(key) as u32);
        let key: Vec<u32> = osh.iter().map(|o| o.text_stroke_color).collect();
        t.shield.set_text_stroke_color(palette.cidx(key) as u32);
    }
}

struct StringTable {
    strings: Vec<String>,
    index: HashMap<String, usize>,
}

impl StringTable {
    fn new() -> Self {
        let mut index = HashMap::new();
        index.insert(String::new(), 0);
        StringTable {
            strings: vec![String::new()],
            index,
        }
    }

    fn sidx(&mut self, s: &str) -> usize {
        if let Some(&i) = self.index.get(s) {
            return i;
        }
        let i = self.strings.len();
        self.strings.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }
}

fn emit_line<T: LineLike>(
    out: &mut Vec<u8>,
    ln: &T,
    with_priority: bool,
    strtab: &mut StringTable,
) {
    let mut flags: u8 = 0;
    if !ln.dashdot().dd.is_empty() {
        flags |= LINE_FLAG_DASHDOT;
    }
    if ln.pathsym().is_set() {
        flags |= LINE_FLAG_PATHSYM;
    }
    out.push(flags);
    write_f32(out, ln.width());
    write_varuint(out, ln.color() as u64);
    if with_priority {
        write_varint(out, ln.priority() as i64);
    }
    out.push(ln.join());
    out.push(ln.cap());
    if flags & LINE_FLAG_DASHDOT != 0 {
        write_varuint(out, ln.dashdot().dd.len() as u64);
        for d in &ln.dashdot().dd {
            write_f32(out, *d);
        }
        write_f32(out, ln.dashdot().offset);
    }
    if flags & LINE_FLAG_PATHSYM != 0 {
        write_varuint(out, strtab.sidx(&ln.pathsym().name) as u64);
        write_f32(out, ln.pathsym().step);
        write_f32(out, ln.pathsym().offset);
    }
}

fn emit_captiondef(out: &mut Vec<u8>, cd: &CaptionDef, strtab: &mut StringTable) {
    write_varint(out, cd.height as i64);
    write_varuint(out, cd.color as u64);
    write_varuint(out, cd.stroke_color as u64);
    write_varint(out, cd.offset_x as i64);
    write_varint(out, cd.offset_y as i64);
    write_varuint(out, strtab.sidx(&cd.text) as u64);
    out.push(if cd.is_optional { 1 } else { 0 });
}

fn emit_caption(out: &mut Vec<u8>, cap: &CaptionRule, strtab: &mut StringTable) {
    let mut flags: u8 = 0;
    if cap.primary.is_set() {
        flags |= CAPTION_FLAG_PRIMARY;
    }
    if cap.secondary.is_set() {
        flags |= CAPTION_FLAG_SECONDARY;
    }
    out.push(flags);
    if flags & CAPTION_FLAG_PRIMARY != 0 {
        emit_captiondef(out, &cap.primary, strtab);
    }
    if flags & CAPTION_FLAG_SECONDARY != 0 {
        emit_captiondef(out, &cap.secondary, strtab);
    }
    write_varint(out, cap.priority as i64);
}

fn emit_element(
    out: &mut Vec<u8>,
    el: &DrawElement,
    strtab: &mut StringTable,
    counts: &mut [u64; 6],
) {
    out.push(el.scale);
    write_varuint(out, el.apply_if.len() as u64);
    for a in &el.apply_if {
        write_varuint(out, strtab.sidx(a) as u64);
    }

    let mut kind: u8 = 0;
    if !el.lines.is_empty() {
        kind |= KIND_LINES;
    }
    if el.area.is_set() {
        kind |= KIND_AREA;
    }
    if el.symbol.is_set() {
        kind |= KIND_SYMBOL;
    }
    if el.caption.is_set() {
        kind |= KIND_CAPTION;
    }
    if el.path_text.is_set() {
        kind |= KIND_PATHTEXT;
    }
    if el.shield.is_set() {
        kind |= KIND_SHIELD;
    }
    out.push(kind);

    if kind & KIND_LINES != 0 {
        write_varuint(out, el.lines.len() as u64);
        for ln in &el.lines {
            emit_line(out, ln, true, strtab);
        }
        counts[0] += el.lines.len() as u64;
    }
    if kind & KIND_AREA != 0 {
        out.push(if el.area.border.is_set() {
            AREA_FLAG_BORDER
        } else {
            0
        });
        write_varuint(out, el.area.color as u64);
        write_varint(out, el.area.priority as i64);
        if el.area.border.is_set() {
            emit_line(out, &el.area.border, false, strtab);
        }
        counts[1] += 1;
    }
    if kind & KIND_SYMBOL != 0 {
        write_varuint(out, strtab.sidx(&el.symbol.name) as u64);
        write_varint(out, el.symbol.apply_for_type as i64);
        write_varint(out, el.symbol.priority as i64);
        write_varint(out, el.symbol.min_distance as i64);
        counts[2] += 1;
    }
    if kind & KIND_CAPTION != 0 {
        emit_caption(out, &el.caption, strtab);
        counts[3] += 1;
    }
    if kind & KIND_PATHTEXT != 0 {
        emit_caption(out, &el.path_text, strtab);
        counts[4] += 1;
    }
    if kind & KIND_SHIELD != 0 {
        let sh = &el.shield;
        write_varint(out, sh.height as i64);
        write_varuint(out, sh.color as u64);
        write_varuint(out, sh.stroke_color as u64);
        write_varint(out, sh.priority as i64);
        write_varint(out, sh.min_distance as i64);
        write_varuint(out, sh.text_color as u64);
        write_varuint(out, sh.text_stroke_color as u64);
        counts[5] += 1;
    }
}

/// Serializes one container per variant (all isomorphic modulo colors) into the
/// native format. For a single variant pass one container.
pub fn serialize_binary(
    containers: &[Container],
    variant_names: &[String],
) -> Result<Vec<u8>, String> {
    if containers.len() != variant_names.len() || containers.is_empty() {
        return Err("expected one non-empty container per variant name".to_string());
    }
    if variant_names.len() > 255 {
        return Err("native drules format supports at most 255 variants".to_string());
    }

    let (template, palette, named) = build_indexed(containers)?;
    let n = variant_names.len();
    let mut strtab = StringTable::new();
    let mut counts = [0u64; 6];

    // Build the string-referencing sections first so the string table is complete afterwards.
    let mut types_buf: Vec<u8> = Vec::new();
    write_varuint(&mut types_buf, template.cont.len() as u64);
    for ce in &template.cont {
        write_varuint(&mut types_buf, strtab.sidx(&ce.name) as u64);
        write_varuint(&mut types_buf, ce.element.len() as u64);
        for el in &ce.element {
            emit_element(&mut types_buf, el, &mut strtab, &mut counts);
        }
    }

    let mut named_buf: Vec<u8> = Vec::new();
    write_varuint(&mut named_buf, named.len() as u64);
    for (name, color_idx) in &named {
        write_varuint(&mut named_buf, strtab.sidx(name) as u64);
        write_varuint(&mut named_buf, *color_idx as u64);
    }

    let mut string_table_buf: Vec<u8> = Vec::new();
    write_varuint(&mut string_table_buf, strtab.strings.len() as u64);
    for s in &strtab.strings {
        write_string(&mut string_table_buf, s);
    }

    let mut color_table_buf: Vec<u8> = Vec::new();
    write_varuint(&mut color_table_buf, palette.len() as u64);
    for v in 0..n {
        for entry in &palette {
            write_u32(&mut color_table_buf, entry[v]);
        }
    }

    let mut rule_counts_buf: Vec<u8> = Vec::new();
    for c in &counts {
        write_varuint(&mut rule_counts_buf, *c);
    }

    let mut overrides_buf: Vec<u8> = Vec::new();
    for _ in 0..n {
        write_varuint(&mut overrides_buf, 0);
    }

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(n as u8);
    for name in variant_names {
        write_string(&mut out, name);
    }
    let sections = [
        &string_table_buf,
        &color_table_buf,
        &named_buf,
        &rule_counts_buf,
        &types_buf,
        &overrides_buf,
    ];
    for section in sections {
        write_varuint(&mut out, section.len() as u64);
        out.extend_from_slice(section);
    }
    Ok(out)
}

fn read_element(r: &mut Reader, strings: &[String]) -> Result<DrawElement, String> {
    let mut el = DrawElement::new();
    el.set_scale(r.u8()?);
    let na = r.varuint()? as usize;
    for _ in 0..na {
        el.apply_if.push(strings[r.varuint()? as usize].clone());
    }
    let kind = r.u8()?;
    if kind & KIND_LINES != 0 {
        let nl = r.varuint()? as usize;
        for _ in 0..nl {
            el.lines.push(read_line(r, strings, true)?);
        }
    }
    if kind & KIND_AREA != 0 {
        let flags = r.u8()?;
        el.area.set_color(r.varuint()? as u32);
        el.area.set_priority(r.varint()? as i32);
        if flags & AREA_FLAG_BORDER != 0 {
            let border = read_line(r, strings, false)?;
            el.area.border.set_width(border.width);
            el.area.border.set_color(border.color);
            el.area.border.set_join(border.join);
            el.area.border.set_cap(border.cap);
            if !border.dashdot.dd.is_empty() {
                el.area.border.dashdot.dd.extend(border.dashdot.dd);
                el.area.border.dashdot.set_offset(border.dashdot.offset);
            }
            if border.pathsym.is_set() {
                el.area.border.pathsym.set_name(border.pathsym.name);
                el.area.border.pathsym.set_step(border.pathsym.step);
                el.area.border.pathsym.set_offset(border.pathsym.offset);
            }
        }
    }
    if kind & KIND_SYMBOL != 0 {
        el.symbol.set_name(strings[r.varuint()? as usize].clone());
        el.symbol.set_apply_for_type(r.varint()? as i32);
        el.symbol.set_priority(r.varint()? as i32);
        el.symbol.set_min_distance(r.varint()? as i32);
    }
    if kind & KIND_CAPTION != 0 {
        read_caption(r, &mut el.caption, strings)?;
    }
    if kind & KIND_PATHTEXT != 0 {
        read_caption(r, &mut el.path_text, strings)?;
    }
    if kind & KIND_SHIELD != 0 {
        el.shield.set_height(r.varint()? as i32);
        el.shield.set_color(r.varuint()? as u32);
        el.shield.set_stroke_color(r.varuint()? as u32);
        el.shield.set_priority(r.varint()? as i32);
        el.shield.set_min_distance(r.varint()? as i32);
        el.shield.set_text_color(r.varuint()? as u32);
        el.shield.set_text_stroke_color(r.varuint()? as u32);
    }
    Ok(el)
}

fn read_line(r: &mut Reader, strings: &[String], with_priority: bool) -> Result<LineRule, String> {
    let mut ln = LineRule::new();
    let flags = r.u8()?;
    ln.set_width(r.f32()?);
    ln.set_color(r.varuint()? as u32);
    if with_priority {
        ln.set_priority(r.varint()? as i32);
    }
    ln.set_join(r.u8()?);
    ln.set_cap(r.u8()?);
    if flags & LINE_FLAG_DASHDOT != 0 {
        let nd = r.varuint()? as usize;
        for _ in 0..nd {
            ln.dashdot.dd.push(r.f32()?);
        }
        ln.dashdot.set_offset(r.f32()?);
    }
    if flags & LINE_FLAG_PATHSYM != 0 {
        ln.pathsym.set_name(strings[r.varuint()? as usize].clone());
        ln.pathsym.set_step(r.f32()?);
        ln.pathsym.set_offset(r.f32()?);
    }
    Ok(ln)
}

fn read_caption(r: &mut Reader, cap: &mut CaptionRule, strings: &[String]) -> Result<(), String> {
    let flags = r.u8()?;
    if flags & CAPTION_FLAG_PRIMARY != 0 {
        read_captiondef(r, &mut cap.primary, strings)?;
    }
    if flags & CAPTION_FLAG_SECONDARY != 0 {
        read_captiondef(r, &mut cap.secondary, strings)?;
    }
    cap.set_priority(r.varint()? as i32);
    Ok(())
}

fn read_captiondef(r: &mut Reader, cd: &mut CaptionDef, strings: &[String]) -> Result<(), String> {
    cd.set_height(r.varint()? as i32);
    cd.set_color(r.varuint()? as u32);
    cd.set_stroke_color(r.varuint()? as u32);
    cd.set_offset_x(r.varint()? as i32);
    cd.set_offset_y(r.varint()? as i32);
    cd.set_text(strings[r.varuint()? as usize].clone());
    cd.set_is_optional(r.u8()? != 0);
    Ok(())
}

/// Rebuilds a Container for one variant, replacing palette indices with this
/// variant's color values.
fn resolve(
    types: &[(String, Vec<DrawElement>)],
    named: &[(usize, usize)],
    strings: &[String],
    palette: &[u32],
) -> Container {
    let mut container = Container::new();
    for &(name_idx, color_idx) in named {
        let mut ce = ColorElement::new();
        ce.set_name(strings[name_idx].as_str());
        ce.set_color(palette[color_idx]);
        container.colors.value.push(ce);
    }
    for (type_name, elements) in types {
        let mut ce = ClassifElement::new();
        ce.set_name(type_name);
        for src in elements {
            ce.element.push(resolve_element(src, palette));
        }
        container.cont.push(ce);
    }
    container
}

fn resolve_element(src: &DrawElement, palette: &[u32]) -> DrawElement {
    let mut el = src.clone();
    for ln in &mut el.lines {
        ln.color = palette[ln.color as usize];
    }
    if el.area.is_set() {
        el.area.color = palette[el.area.color as usize];
        if el.area.border.is_set() {
            el.area.border.color = palette[el.area.border.color as usize];
        }
    }
    for cap in [&mut el.caption, &mut el.path_text] {
        if cap.is_set() {
            for sub in [&mut cap.primary, &mut cap.secondary] {
                if sub.is_set() {
                    sub.color = palette[sub.color as usize];
                    sub.stroke_color = palette[sub.stroke_color as usize];
                }
            }
        }
    }
    if el.shield.is_set() {
        el.shield.color = palette[el.shield.color as usize];
        el.shield.stroke_color = palette[el.shield.stroke_color as usize];
        el.shield.text_color = palette[el.shield.text_color as usize];
        el.shield.text_stroke_color = palette[el.shield.text_stroke_color as usize];
    }
    el
}

/// Parses a native blob into `(variant_names, [container per variant])`; each
/// container holds the resolved color VALUES of its variant.
pub fn parse_binary(data: &[u8]) -> Result<(Vec<String>, Vec<Container>), String> {
    let mut r = Reader { data, pos: 0 };
    if r.raw(MAGIC.len())? != MAGIC {
        return Err("Not a drules file (bad magic)".to_string());
    }
    let version = r.u8()?;
    if version != VERSION {
        return Err(format!("Unsupported drules format version {version}"));
    }
    let n = r.u8()? as usize;
    let mut variant_names = Vec::with_capacity(n);
    for _ in 0..n {
        variant_names.push(r.string()?);
    }

    let begin_section = |r: &mut Reader| -> Result<usize, String> {
        let size = r.varuint()? as usize;
        let end = r.pos + size;
        if end > r.data.len() {
            return Err("Truncated drules section".to_string());
        }
        Ok(end)
    };
    let end_section = |r: &mut Reader, end: usize| -> Result<(), String> {
        if r.pos > end {
            return Err("Drules section overrun".to_string());
        }
        r.pos = end;
        Ok(())
    };

    // String table.
    let end = begin_section(&mut r)?;
    let nstr = r.varuint()? as usize;
    let mut strings = Vec::with_capacity(nstr);
    for _ in 0..nstr {
        strings.push(r.string()?);
    }
    if strings.is_empty() || !strings[0].is_empty() {
        return Err("String table must start with the empty string".to_string());
    }
    end_section(&mut r, end)?;

    // Color table: palette[variant][idx].
    let end = begin_section(&mut r)?;
    let color_count = r.varuint()? as usize;
    let mut palette = vec![vec![0u32; color_count]; n];
    for row in palette.iter_mut() {
        for cell in row.iter_mut() {
            *cell = r.u32()?;
        }
    }
    end_section(&mut r, end)?;

    // Named colors.
    let end = begin_section(&mut r)?;
    let named_count = r.varuint()? as usize;
    let mut named = Vec::with_capacity(named_count);
    for _ in 0..named_count {
        named.push((r.varuint()? as usize, r.varuint()? as usize));
    }
    end_section(&mut r, end)?;

    // Rule counts (only needed by the C++ loader for reserve()).
    let end = begin_section(&mut r)?;
    let mut _rule_counts = [0u64; 6];
    for cnt in _rule_counts.iter_mut() {
        *cnt = r.varuint()?;
    }
    end_section(&mut r, end)?;

    // Types: parse into a variant-independent intermediate (color fields stay
    // as palette indices).
    let end = begin_section(&mut r)?;
    let mut types: Vec<(String, Vec<DrawElement>)> = Vec::new();
    let nt = r.varuint()? as usize;
    for _ in 0..nt {
        let name_idx = r.varuint()? as usize;
        let ne = r.varuint()? as usize;
        let mut elements = Vec::with_capacity(ne);
        for _ in 0..ne {
            elements.push(read_element(&mut r, &strings)?);
        }
        types.push((strings[name_idx].clone(), elements));
    }
    end_section(&mut r, end)?;

    // Overrides are currently always empty; parse and ignore.
    let end = begin_section(&mut r)?;
    for _ in 0..n {
        let no = r.varuint()? as usize;
        for _ in 0..no {
            r.varuint()?; // typeIdx
            r.varuint()?; // elemIdx
            read_element(&mut r, &strings)?;
        }
    }
    end_section(&mut r, end)?;

    let mut containers = Vec::with_capacity(n);
    for p in &palette {
        containers.push(resolve(&types, &named, &strings, p));
    }
    Ok((variant_names, containers))
}

fn color_id(idx: usize) -> String {
    format!("c{:03}", idx)
}

fn fmt_g(v: f64) -> String {
    g_format(v, 6)
}

fn is_transparent(palette: &[Vec<u32>], color_idx: usize) -> bool {
    palette[color_idx].iter().all(|&v| v == 0)
}

fn cap_str(c: u8) -> String {
    match c {
        ROUND_CAP => "round".to_string(),
        BUTT_CAP => "butt".to_string(),
        SQUARE_CAP => "square".to_string(),
        _ => c.to_string(),
    }
}

fn join_str(j: u8) -> String {
    match j {
        ROUND_JOIN => "round".to_string(),
        BEVEL_JOIN => "bevel".to_string(),
        NO_JOIN => "no".to_string(),
        _ => j.to_string(),
    }
}

fn line_str<T: LineLike>(ln: &T, with_priority: bool) -> String {
    let mut parts = vec![
        format!("width={}", fmt_g(ln.width())),
        format!("color={}", color_id(ln.color() as usize)),
        format!("join={}", join_str(ln.join())),
        format!("cap={}", cap_str(ln.cap())),
    ];
    if !ln.dashdot().dd.is_empty() {
        let dash: Vec<String> = ln.dashdot().dd.iter().map(|d| fmt_g(*d)).collect();
        parts.push(format!("dash=[{}]", dash.join(",")));
        if ln.dashdot().offset != 0.0 {
            parts.push(format!("dash_offset={}", fmt_g(ln.dashdot().offset)));
        }
    }
    if ln.pathsym().is_set() {
        parts.push(format!(
            "pathsym={} step={} offset={}",
            ln.pathsym().name,
            fmt_g(ln.pathsym().step),
            fmt_g(ln.pathsym().offset)
        ));
    }
    if with_priority {
        parts.push(format!("priority={}", ln.priority()));
    }
    parts.join(" ")
}

fn captiondef_str(cd: &CaptionDef, palette: &[Vec<u32>]) -> String {
    let mut parts = vec![
        format!("height={}", cd.height),
        format!("color={}", color_id(cd.color as usize)),
    ];
    if !is_transparent(palette, cd.stroke_color as usize) {
        parts.push(format!("stroke={}", color_id(cd.stroke_color as usize)));
    }
    if cd.offset_x != 0 {
        parts.push(format!("dx={}", cd.offset_x));
    }
    if cd.offset_y != 0 {
        parts.push(format!("dy={}", cd.offset_y));
    }
    if !cd.text.is_empty() {
        parts.push(format!("text={}", repr_str(&cd.text)));
    }
    if cd.is_optional {
        parts.push("optional".to_string());
    }
    parts.join(" ")
}

fn caption_str(keyword: &str, cap_rule: &CaptionRule, palette: &[Vec<u32>]) -> String {
    let mut parts = vec![keyword.to_string()];
    if cap_rule.primary.is_set() {
        parts.push(format!(
            "primary[{}]",
            captiondef_str(&cap_rule.primary, palette)
        ));
    }
    if cap_rule.secondary.is_set() {
        parts.push(format!(
            "secondary[{}]",
            captiondef_str(&cap_rule.secondary, palette)
        ));
    }
    parts.push(format!("priority={}", cap_rule.priority));
    parts.join(" ")
}

/// Produces the canonical, deterministic, review-only text dump (all variants
/// side by side).
pub fn serialize_text(
    containers: &[Container],
    variant_names: &[String],
) -> Result<String, String> {
    let (template, palette, named) = build_indexed(containers)?;
    let mut out: Vec<String> = Vec::new();
    out.push(
        "# drules text dump, format 1. Generated by generate_drules.sh - do not edit.".to_string(),
    );
    out.push(format!("variants: {}", variant_names.join(" ")));
    out.push("colors:".to_string());
    for (idx, entry) in palette.iter().enumerate() {
        let vals: Vec<String> = entry.iter().map(|v| format!("#{:08X}", v)).collect();
        out.push(format!("  {} {}", color_id(idx), vals.join(" ")));
    }
    if !named.is_empty() {
        out.push("named-colors:".to_string());
        for (name, color_idx) in &named {
            out.push(format!("  {} {}", name, color_id(*color_idx)));
        }
    }

    for ce in &template.cont {
        out.push(format!("type {}", ce.name));
        for el in &ce.element {
            let mut head = format!("  z{}", el.scale);
            if !el.apply_if.is_empty() {
                let quoted: Vec<String> =
                    el.apply_if.iter().map(|a| format!("\"{}\"", a)).collect();
                head += &format!(" if {}", quoted.join(" and "));
            }
            out.push(head);
            for ln in &el.lines {
                out.push(format!("    line {}", line_str(ln, true)));
            }
            if el.area.is_set() {
                let mut parts = vec![format!("color={}", color_id(el.area.color as usize))];
                if el.area.border.is_set() {
                    parts.push(format!("border[{}]", line_str(&el.area.border, false)));
                }
                parts.push(format!("priority={}", el.area.priority));
                out.push(format!("    area {}", parts.join(" ")));
            }
            if el.symbol.is_set() {
                let mut parts = vec![
                    format!("name={}", el.symbol.name),
                    format!("priority={}", el.symbol.priority),
                ];
                if el.symbol.apply_for_type != 0 {
                    parts.push(format!("apply_for_type={}", el.symbol.apply_for_type));
                }
                if el.symbol.min_distance != 0 {
                    parts.push(format!("min_distance={}", el.symbol.min_distance));
                }
                out.push(format!("    symbol {}", parts.join(" ")));
            }
            if el.caption.is_set() {
                out.push(format!(
                    "    {}",
                    caption_str("caption", &el.caption, &palette)
                ));
            }
            if el.path_text.is_set() {
                out.push(format!(
                    "    {}",
                    caption_str("path_text", &el.path_text, &palette)
                ));
            }
            if el.shield.is_set() {
                let sh = &el.shield;
                let mut parts = vec![
                    format!("height={}", sh.height),
                    format!("color={}", color_id(sh.color as usize)),
                ];
                if !is_transparent(&palette, sh.stroke_color as usize) {
                    parts.push(format!("stroke={}", color_id(sh.stroke_color as usize)));
                }
                parts.push(format!("text_color={}", color_id(sh.text_color as usize)));
                if !is_transparent(&palette, sh.text_stroke_color as usize) {
                    parts.push(format!(
                        "text_stroke={}",
                        color_id(sh.text_stroke_color as usize)
                    ));
                }
                parts.push(format!("priority={}", sh.priority));
                if sh.min_distance != 0 {
                    parts.push(format!("min_distance={}", sh.min_distance));
                }
                out.push(format!("    shield {}", parts.join(" ")));
            }
        }
    }

    Ok(out.join("\n") + "\n")
}

pub fn load_all(path: &Path) -> Result<(Vec<String>, Vec<Container>), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    parse_binary(&data)
}

/// Loads a native file and returns its first variant's container (handy for
/// single-variant files and for tools that only inspect the shared structure).
pub fn load_container(path: &Path) -> Result<Container, String> {
    Ok(load_all(path)?
        .1
        .into_iter()
        .next()
        .ok_or("No variants in drules file")?)
}

pub fn save_binary(
    path: &Path,
    containers: &[Container],
    variant_names: &[String],
) -> Result<(), String> {
    std::fs::write(path, serialize_binary(containers, variant_names)?).map_err(|e| e.to_string())
}

pub fn save_text(
    path: &Path,
    containers: &[Container],
    variant_names: &[String],
) -> Result<(), String> {
    std::fs::write(path, serialize_text(containers, variant_names)?.as_bytes())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            rule.element.extend(elements.iter().cloned());
            container.cont.push(rule);
        }
        container
    }

    fn read_varuint(data: &[u8], mut pos: usize) -> (u64, usize) {
        let mut shift = 0;
        let mut result = 0u64;
        loop {
            let b = data[pos];
            pos += 1;
            result |= ((b & 127) as u64) << shift;
            if b & 128 == 0 {
                return (result, pos);
            }
            shift += 7;
        }
    }

    fn encode_varuint(value: u64) -> Vec<u8> {
        let mut result = Vec::new();
        write_varuint(&mut result, value);
        result
    }

    #[test]
    fn test_parse_binary_skips_section_padding() {
        let container = make_container(&[("highway-primary", vec![make_element(10, 2.0)])]);
        let mut blob = serialize_binary(&[container], &[String::from("light")]).unwrap();

        let mut pos = MAGIC.len() + 1 + 1;
        let (variant_name_len, p) = read_varuint(&blob, pos);
        pos = p + variant_name_len as usize;

        let section_size_pos = pos;
        let (section_size, section_body_pos) = read_varuint(&blob, section_size_pos);
        let mut section_end = section_body_pos + section_size as usize;

        let encoded_size = encode_varuint(section_size + 1);
        let old_len = section_body_pos - section_size_pos;
        blob.splice(
            section_size_pos..section_body_pos,
            encoded_size.iter().cloned(),
        );
        section_end += encoded_size.len() - old_len;
        blob.insert(section_end, 0);

        let (variant_names, containers) = parse_binary(&blob).unwrap();
        assert_eq!(variant_names, vec!["light"]);
        assert_eq!(containers[0].cont.len(), 1);
    }

    #[test]
    fn test_binary_roundtrip_is_identity() {
        let container = make_container(&[
            ("highway-primary", vec![make_element(10, 2.0)]),
            ("type-a", vec![make_element(5, 1.5), make_element(7, 4.0)]),
        ]);
        let blob =
            serialize_binary(std::slice::from_ref(&container), &["design".to_string()]).unwrap();
        let (names, containers) = parse_binary(&blob).unwrap();
        assert_eq!(names, vec!["design"]);
        let blob2 = serialize_binary(&containers, &names).unwrap();
        assert_eq!(blob, blob2);
    }

    #[test]
    fn test_multi_variant_pack_and_roundtrip() {
        let light = make_container(&[("highway", vec![make_element(6, 2.0)])]);
        let dark = make_container(&[("highway", vec![make_element(6, 2.0)])]);
        let blob = serialize_binary(
            &[light, dark],
            &[String::from("light"), String::from("dark")],
        )
        .unwrap();
        let (names, containers) = parse_binary(&blob).unwrap();
        assert_eq!(names, vec!["light", "dark"]);
        let blob2 = serialize_binary(&containers, &names).unwrap();
        assert_eq!(blob, blob2);
    }

    #[test]
    fn test_non_isomorphic_variants_error() {
        let light = make_container(&[("highway", vec![make_element(6, 2.0)])]);
        let mut dark = make_container(&[("highway", vec![make_element(6, 2.0)])]);
        // Dark variant has an extra element; non-isomorphic modulo colors.
        dark.cont[0].element.push(make_element(7, 3.0));
        let err = serialize_binary(
            &[light, dark],
            &[String::from("light"), String::from("dark")],
        )
        .unwrap_err();
        assert!(err.contains("not isomorphic"));
    }

    #[test]
    fn test_canonical_string() {
        let mut el = make_element(10, 2.0);
        el.lines[0].set_priority(7);
        el.apply_if
            .extend(["name".to_string(), "highway".to_string()]);
        let mut line2 = LineRule::new();
        line2.set_width(0.0);
        line2.set_color(0u32);
        line2.pathsym.set_name("arrow.svg");
        line2.pathsym.set_step(24.0);
        line2.pathsym.set_offset(1.5);
        line2.set_priority(3);
        el.lines.push(line2);
        let s = el.canonical(false);
        assert_eq!(
            s,
            "scale=10,apply_if=['name', 'highway'],lines=[width=2.0,color=4278256131,priority=7,priority=3,pathsym{name='arrow.svg',step=24.0,offset=1.5}]"
        );
    }

    #[test]
    fn test_default_assignment_marks_present() {
        // Assigning a scalar equal to its default still marks the message "set"
        // (mirrors _Msg.__setattr__), e.g. is_optional = False.
        let mut cap = CaptionDef::new();
        cap.set_is_optional(false);
        assert!(cap.is_set());
        assert_eq!(cap.canonical(false), "");
        let fresh = CaptionDef::new();
        assert!(!fresh.is_set());
    }
}
