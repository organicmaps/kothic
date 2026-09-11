"""Native drawing-rules format used to compile MapCSS styles.

It provides:
  * builder classes (Container, ClassifElement, ...) whose API mirrors what libkomwm.py expects
    (attribute assignment, eager sub-rules, repeated .extend()/.append(), str() for
    de-duplication);
  * a binary writer/reader for the native format, and a canonical text dumper.

The wire format MUST stay in sync with libs/indexer/drules_format.{hpp,cpp}.

Color model: builder objects hold actual ARGB color VALUES, so merge tools can copy rules around
freely. The palette+index encoding is an internal detail of the binary writer
(values -> indices) and reader (indices -> values). A family file packs several variants (e.g.
light/dark) that share one structure and differ only in their color palette columns.
"""

import copy
import struct

__all__ = [
    "Container", "ClassifElement", "DrawElement", "LineRule", "LineDef",
    "AreaRule", "SymbolRule", "CaptionRule", "PathTextRule", "CaptionDef",
    "ShieldRule", "DashDot", "PathSym", "ColorElement", "ColorsElement",
    "ROUNDCAP", "BUTTCAP", "SQUARECAP", "ROUNDJOIN", "BEVELJOIN", "NOJOIN",
    "serialize_binary", "serialize_text", "parse_binary",
    "load_all", "load_container", "save_binary", "save_text",
]

# --- Wire constants (keep in sync with libs/indexer/drules_format.hpp) ---
MAGIC = b"OMDR"
VERSION = 1

LINE_FLAG_DASHDOT = 1 << 0
LINE_FLAG_PATHSYM = 1 << 1
AREA_FLAG_BORDER = 1 << 0
CAPTION_FLAG_PRIMARY = 1 << 0
CAPTION_FLAG_SECONDARY = 1 << 1

KIND_LINES = 1 << 0
KIND_AREA = 1 << 1
KIND_SYMBOL = 1 << 2
KIND_CAPTION = 1 << 3
KIND_PATHTEXT = 1 << 4
KIND_SHIELD = 1 << 5

# LineCap / LineJoin enum values (match drules_struct.hpp). Exported for libkomwm.py.
ROUNDCAP, BUTTCAP, SQUARECAP = 0, 1, 2
ROUNDJOIN, BEVELJOIN, NOJOIN = 0, 1, 2

# The per-variant color scalars (everything else is shared across a family's variants).
_COLOR_FIELDS = frozenset(("color", "stroke_color", "text_color", "text_stroke_color"))


# ------------------------------- message classes -------------------------------

class _Msg:
    """Minimal draw-rule builder base.

    Subclasses declare fields via class attributes:
      _SCALARS        : dict name -> default value
      _SUBMSGS        : dict name -> child class (singular, eagerly created)
      _REPEATED_SCALAR: tuple of repeated-scalar field names (plain lists)
      _REPEATED_MSG   : dict name -> repeated child class (plain lists)

    A child is considered "set" once any of its scalars was assigned (tracked by _present), one of
    its repeated fields is non-empty, or one of its own children is set (see _is_set()). Merely
    reading a field never sets presence, so the `if dr_element.symbol.priority:` idiom in
    libkomwm.py keeps working.
    """

    _SCALARS = {}
    _SUBMSGS = {}
    _REPEATED_SCALAR = ()
    _REPEATED_MSG = {}

    def __init__(self):
        d = self.__dict__
        d["_present"] = False
        for name, default in self._SCALARS.items():
            d[name] = default
        for name, cls in self._SUBMSGS.items():
            d[name] = cls()
        for name in self._REPEATED_SCALAR:
            d[name] = []
        for name in self._REPEATED_MSG:
            d[name] = []

    def __setattr__(self, name, value):
        self.__dict__[name] = value
        if name in self._SCALARS:
            self.__dict__["_present"] = True

    def _is_set(self):
        if self._present:
            return True
        for name in self._REPEATED_SCALAR:
            if self.__dict__[name]:
                return True
        for name in self._REPEATED_MSG:
            if self.__dict__[name]:
                return True
        for name in self._SUBMSGS:
            if self.__dict__[name]._is_set():
                return True
        return False

    def _canonical(self, skip_colors=False):
        """A deterministic content string used as a de-duplication key: two builders with equal
        emitted content produce equal strings. With skip_colors the four color fields are omitted,
        giving a structure-only key for the light/dark isomorphism check (skipping a color is
        equivalent to zeroing it, since 0 is the default and defaults are already omitted)."""
        parts = []
        for name, default in self._SCALARS.items():
            if skip_colors and name in _COLOR_FIELDS:
                continue
            v = self.__dict__[name]
            if v != default:
                parts.append(f"{name}={v!r}")
        for name in self._REPEATED_SCALAR:
            v = self.__dict__[name]
            if v:
                parts.append(f"{name}={list(v)!r}")
        for name in self._SUBMSGS:
            sub = self.__dict__[name]
            if sub._is_set():
                parts.append(f"{name}{{{sub._canonical(skip_colors)}}}")
        for name in self._REPEATED_MSG:
            lst = self.__dict__[name]
            if lst:
                parts.append(f"{name}=[{','.join(m._canonical(skip_colors) for m in lst)}]")
        return ",".join(parts)

    def __str__(self):
        return self._canonical()

    def __deepcopy__(self, memo):
        # copy.deepcopy is ~90% of serialize/parse time on the generic path; this tree is acyclic
        # and holds only scalars, child messages and lists of those, so a direct walk is far cheaper.
        new = self.__class__.__new__(self.__class__)
        memo[id(self)] = new
        d = new.__dict__
        for k, v in self.__dict__.items():
            if isinstance(v, _Msg):
                d[k] = v.__deepcopy__(memo)
            elif isinstance(v, list):
                d[k] = [e.__deepcopy__(memo) if isinstance(e, _Msg) else e for e in v]
            else:
                d[k] = v  # scalars (int/float/str/bool) are immutable
        return new


class DashDot(_Msg):
    _SCALARS = {"offset": 0.0}
    _REPEATED_SCALAR = ("dd",)


class PathSym(_Msg):
    _SCALARS = {"name": "", "step": 0.0, "offset": 0.0}


class LineDef(_Msg):
    _SCALARS = {"width": 0.0, "color": 0, "join": ROUNDJOIN, "cap": ROUNDCAP}
    _SUBMSGS = {"dashdot": DashDot, "pathsym": PathSym}


class LineRule(_Msg):
    _SCALARS = {"width": 0.0, "color": 0, "priority": 0, "join": ROUNDJOIN, "cap": ROUNDCAP}
    _SUBMSGS = {"dashdot": DashDot, "pathsym": PathSym}


class AreaRule(_Msg):
    _SCALARS = {"color": 0, "priority": 0}
    _SUBMSGS = {"border": LineDef}


class SymbolRule(_Msg):
    _SCALARS = {"name": "", "apply_for_type": 0, "priority": 0, "min_distance": 0}


class CaptionDef(_Msg):
    _SCALARS = {"height": 0, "color": 0, "stroke_color": 0, "offset_x": 0, "offset_y": 0,
                "text": "", "is_optional": False}


class CaptionRule(_Msg):
    _SCALARS = {"priority": 0}
    _SUBMSGS = {"primary": CaptionDef, "secondary": CaptionDef}


# PathText has the exact same shape as Caption.
PathTextRule = CaptionRule


class ShieldRule(_Msg):
    _SCALARS = {"height": 0, "color": 0, "stroke_color": 0, "priority": 0, "min_distance": 0,
                "text_color": 0, "text_stroke_color": 0}


class DrawElement(_Msg):
    _SCALARS = {"scale": 0}
    _REPEATED_SCALAR = ("apply_if",)
    _REPEATED_MSG = {"lines": LineRule}
    _SUBMSGS = {"area": AreaRule, "symbol": SymbolRule, "caption": CaptionRule,
                "path_text": PathTextRule, "shield": ShieldRule}


class ClassifElement(_Msg):
    _SCALARS = {"name": ""}
    _REPEATED_MSG = {"element": DrawElement}


class ColorElement(_Msg):
    _SCALARS = {"name": "", "color": 0}


class ColorsElement(_Msg):
    _REPEATED_MSG = {"value": ColorElement}


class Container(_Msg):
    _SUBMSGS = {"colors": ColorsElement}
    _REPEATED_MSG = {"cont": ClassifElement}


# ------------------------------- byte helpers -------------------------------

def _write_varuint(buf, value):
    assert value >= 0
    while value > 127:
        buf.append((value & 127) | 128)
        value >>= 7
    buf.append(value)


def _write_varint(buf, value):
    # ZigZag of a signed int32 (the C++ reader reads these via ReadVarInt<int32_t>), matching
    # coding/varint.hpp WriteVarInt + bits::ZigZagEncode.
    assert -(1 << 31) <= value < (1 << 31), value
    _write_varuint(buf, (value << 1) ^ (value >> 63) if value < 0 else value << 1)


def _write_f32(buf, value):
    buf += struct.pack("<f", value)


def _write_u32(buf, value):
    buf += struct.pack("<I", value & 0xFFFFFFFF)


def _write_string(buf, s):
    data = s.encode("utf-8")
    _write_varuint(buf, len(data))
    buf += data


class _Reader:
    def __init__(self, data):
        self.data = data
        self.pos = 0

    def u8(self):
        b = self.data[self.pos]
        self.pos += 1
        return b

    def varuint(self):
        shift = 0
        result = 0
        while True:
            b = self.data[self.pos]
            self.pos += 1
            result |= (b & 127) << shift
            if not (b & 128):
                return result
            shift += 7

    def varint(self):
        u = self.varuint()
        return (u >> 1) ^ -(u & 1)

    def f32(self):
        v = struct.unpack_from("<f", self.data, self.pos)[0]
        self.pos += 4
        return v

    def u32(self):
        v = struct.unpack_from("<I", self.data, self.pos)[0]
        self.pos += 4
        return v

    def string(self):
        n = self.varuint()
        s = self.data[self.pos:self.pos + n].decode("utf-8")
        self.pos += n
        return s

    def raw(self, n):
        b = self.data[self.pos:self.pos + n]
        self.pos += n
        return b


# ------------------------------- family packing -------------------------------

def _build_indexed(containers):
    """Walks the variant containers in lockstep and returns (template, palette, named):
      * template: a copy of containers[0] with every color scalar replaced by its palette index;
      * palette : list of per-variant color tuples (palette[idx] == (val_variant0, val_variant1...));
      * named   : list of (name, palette_index) for the colors{} block.
    Raises ValueError unless all variants are isomorphic modulo the four color fields."""
    base = containers[0]._canonical(skip_colors=True)
    for c in containers[1:]:
        if c._canonical(skip_colors=True) != base:
            raise ValueError("drules variants are not isomorphic modulo colors; "
                             "per-variant overrides are not supported yet")

    template = copy.deepcopy(containers[0])
    palette = []
    palette_index = {}

    def cidx(objs, field):
        key = tuple(getattr(o, field) for o in objs)
        idx = palette_index.get(key)
        if idx is None:
            idx = len(palette)
            palette_index[key] = idx
            palette.append(key)
        return idx

    def index_linedef(t, objs):
        t.color = cidx(objs, "color")

    def index_caption(t, objs):  # CaptionRule / PathTextRule
        for sub in ("primary", "secondary"):
            tdef = getattr(t, sub)
            if tdef._is_set():
                odefs = [getattr(o, sub) for o in objs]
                tdef.color = cidx(odefs, "color")
                tdef.stroke_color = cidx(odefs, "stroke_color")

    def index_element(t, objs):
        for k, tl in enumerate(t.lines):
            index_linedef(tl, [o.lines[k] for o in objs])
        if t.area._is_set():
            oa = [o.area for o in objs]
            t.area.color = cidx(oa, "color")
            if t.area.border._is_set():
                index_linedef(t.area.border, [o.border for o in oa])
        if t.caption._is_set():
            index_caption(t.caption, [o.caption for o in objs])
        if t.path_text._is_set():
            index_caption(t.path_text, [o.path_text for o in objs])
        if t.shield._is_set():
            osh = [o.shield for o in objs]
            for f in ("color", "stroke_color", "text_color", "text_stroke_color"):
                setattr(t.shield, f, cidx(osh, f))

    named = []
    for m, ce in enumerate(template.colors.value):
        named.append((ce.name, cidx([c.colors.value[m] for c in containers], "color")))

    for i, tc in enumerate(template.cont):
        ocs = [c.cont[i] for c in containers]
        for j, te in enumerate(tc.element):
            index_element(te, [oc.element[j] for oc in ocs])

    return template, palette, named


# ------------------------------- binary writer -------------------------------

def serialize_binary(containers, variant_names):
    """Serializes one container per variant (all isomorphic modulo colors) into the native format.
    For a single variant pass one container, e.g. serialize_binary([c], ['design'])."""
    if len(containers) != len(variant_names) or not containers:
        raise ValueError("expected one non-empty container per variant name")
    if len(variant_names) > 255:
        raise ValueError("native drules format supports at most 255 variants")

    template, palette, named = _build_indexed(containers)
    n = len(variant_names)

    strings = [""]
    str_index = {"": 0}

    def sidx(s):
        i = str_index.get(s)
        if i is None:
            i = len(strings)
            str_index[s] = i
            strings.append(s)
        return i

    counts = [0] * 6  # lines, areas, symbols, captions, pathtexts, shields

    def emit_line(buf, ln, with_priority):
        flags = 0
        if ln.dashdot.dd:
            flags |= LINE_FLAG_DASHDOT
        if ln.pathsym._is_set():
            flags |= LINE_FLAG_PATHSYM
        buf.append(flags)
        _write_f32(buf, ln.width)
        _write_varuint(buf, ln.color)
        if with_priority:
            _write_varint(buf, ln.priority)
        buf.append(ln.join)
        buf.append(ln.cap)
        if flags & LINE_FLAG_DASHDOT:
            _write_varuint(buf, len(ln.dashdot.dd))
            for d in ln.dashdot.dd:
                _write_f32(buf, d)
            _write_f32(buf, ln.dashdot.offset)
        if flags & LINE_FLAG_PATHSYM:
            _write_varuint(buf, sidx(ln.pathsym.name))
            _write_f32(buf, ln.pathsym.step)
            _write_f32(buf, ln.pathsym.offset)

    def emit_captiondef(buf, cd):
        _write_varint(buf, cd.height)
        _write_varuint(buf, cd.color)
        _write_varuint(buf, cd.stroke_color)
        _write_varint(buf, cd.offset_x)
        _write_varint(buf, cd.offset_y)
        _write_varuint(buf, sidx(cd.text))
        buf.append(1 if cd.is_optional else 0)

    def emit_caption(buf, cap):
        flags = 0
        if cap.primary._is_set():
            flags |= CAPTION_FLAG_PRIMARY
        if cap.secondary._is_set():
            flags |= CAPTION_FLAG_SECONDARY
        buf.append(flags)
        if flags & CAPTION_FLAG_PRIMARY:
            emit_captiondef(buf, cap.primary)
        if flags & CAPTION_FLAG_SECONDARY:
            emit_captiondef(buf, cap.secondary)
        _write_varint(buf, cap.priority)

    def emit_element(buf, el):
        buf.append(el.scale)
        _write_varuint(buf, len(el.apply_if))
        for a in el.apply_if:
            _write_varuint(buf, sidx(a))

        kind = 0
        if el.lines:
            kind |= KIND_LINES
        if el.area._is_set():
            kind |= KIND_AREA
        if el.symbol._is_set():
            kind |= KIND_SYMBOL
        if el.caption._is_set():
            kind |= KIND_CAPTION
        if el.path_text._is_set():
            kind |= KIND_PATHTEXT
        if el.shield._is_set():
            kind |= KIND_SHIELD
        buf.append(kind)

        if kind & KIND_LINES:
            _write_varuint(buf, len(el.lines))
            for ln in el.lines:
                emit_line(buf, ln, with_priority=True)
            counts[0] += len(el.lines)
        if kind & KIND_AREA:
            buf.append(AREA_FLAG_BORDER if el.area.border._is_set() else 0)
            _write_varuint(buf, el.area.color)
            _write_varint(buf, el.area.priority)
            if el.area.border._is_set():
                emit_line(buf, el.area.border, with_priority=False)
            counts[1] += 1
        if kind & KIND_SYMBOL:
            _write_varuint(buf, sidx(el.symbol.name))
            _write_varint(buf, el.symbol.apply_for_type)
            _write_varint(buf, el.symbol.priority)
            _write_varint(buf, el.symbol.min_distance)
            counts[2] += 1
        if kind & KIND_CAPTION:
            emit_caption(buf, el.caption)
            counts[3] += 1
        if kind & KIND_PATHTEXT:
            emit_caption(buf, el.path_text)
            counts[4] += 1
        if kind & KIND_SHIELD:
            sh = el.shield
            _write_varint(buf, sh.height)
            _write_varuint(buf, sh.color)
            _write_varuint(buf, sh.stroke_color)
            _write_varint(buf, sh.priority)
            _write_varint(buf, sh.min_distance)
            _write_varuint(buf, sh.text_color)
            _write_varuint(buf, sh.text_stroke_color)
            counts[5] += 1

    # Build the string-referencing sections first so the string table is complete afterwards.
    types_buf = bytearray()
    _write_varuint(types_buf, len(template.cont))
    for ce in template.cont:
        _write_varuint(types_buf, sidx(ce.name))
        _write_varuint(types_buf, len(ce.element))
        for el in ce.element:
            emit_element(types_buf, el)

    named_buf = bytearray()
    _write_varuint(named_buf, len(named))
    for name, color_idx in named:
        _write_varuint(named_buf, sidx(name))
        _write_varuint(named_buf, color_idx)

    string_table_buf = bytearray()
    _write_varuint(string_table_buf, len(strings))
    for s in strings:
        _write_string(string_table_buf, s)

    color_table_buf = bytearray()
    _write_varuint(color_table_buf, len(palette))
    for v in range(n):
        for entry in palette:
            _write_u32(color_table_buf, entry[v])

    rule_counts_buf = bytearray()
    for c in counts:
        _write_varuint(rule_counts_buf, c)

    overrides_buf = bytearray()
    for _ in range(n):
        _write_varuint(overrides_buf, 0)

    out = bytearray()
    out += MAGIC
    out.append(VERSION)
    out.append(n)
    for name in variant_names:
        _write_string(out, name)
    for section in (string_table_buf, color_table_buf, named_buf, rule_counts_buf, types_buf, overrides_buf):
        _write_varuint(out, len(section))
        out += section
    return bytes(out)


# ------------------------------- binary reader -------------------------------

def parse_binary(data):
    """Parses a native blob into (variant_names, [container per variant]); each container holds the
    resolved color VALUES of its variant."""
    r = _Reader(data)
    if r.raw(len(MAGIC)) != MAGIC:
        raise ValueError("Not a drules file (bad magic)")
    version = r.u8()
    if version != VERSION:
        raise ValueError(f"Unsupported drules format version {version}")
    n = r.u8()
    variant_names = [r.string() for _ in range(n)]

    def begin_section():
        size = r.varuint()
        end = r.pos + size
        if end > len(r.data):
            raise ValueError("Truncated drules section")
        return end

    def end_section(end):
        if r.pos > end:
            raise ValueError("Drules section overrun")
        r.pos = end

    # String table.
    end = begin_section()
    strings = [r.string() for _ in range(r.varuint())]
    if not strings or strings[0] != "":
        raise ValueError("String table must start with the empty string")
    end_section(end)

    # Color table: palette[variant][idx].
    end = begin_section()
    color_count = r.varuint()
    palette = [[r.u32() for _ in range(color_count)] for _ in range(n)]
    end_section(end)

    # Named colors.
    end = begin_section()
    named = [(r.varuint(), r.varuint()) for _ in range(r.varuint())]  # (nameIdx, colorIdx)
    end_section(end)

    # Rule counts (only needed by the C++ loader for reserve()).
    end = begin_section()
    _rule_counts = [r.varuint() for _ in range(6)]
    end_section(end)

    # Types: parse into a variant-independent intermediate (color fields stay as palette indices).
    end = begin_section()
    types = []
    for _ in range(r.varuint()):
        name_idx = r.varuint()
        elements = [_read_element(r, strings) for _ in range(r.varuint())]
        types.append((strings[name_idx], elements))
    end_section(end)

    # Overrides are currently always empty; parse and ignore.
    end = begin_section()
    for _ in range(n):
        for _ in range(r.varuint()):
            r.varuint()  # typeIdx
            r.varuint()  # elemIdx
            _read_element(r, strings)
    end_section(end)

    containers = [_resolve(types, named, strings, palette[v]) for v in range(n)]
    return variant_names, containers


def _read_element(r, strings):
    el = DrawElement()
    el.scale = r.u8()
    el.apply_if.extend(strings[r.varuint()] for _ in range(r.varuint()))
    kind = r.u8()
    if kind & KIND_LINES:
        for _ in range(r.varuint()):
            el.lines.append(_read_line(r, strings, with_priority=True))
    if kind & KIND_AREA:
        flags = r.u8()
        el.area.color = r.varuint()
        el.area.priority = r.varint()
        if flags & AREA_FLAG_BORDER:
            border = _read_line(r, strings, with_priority=False)
            el.area.border.width = border.width
            el.area.border.color = border.color
            el.area.border.join = border.join
            el.area.border.cap = border.cap
            if border.dashdot.dd:
                el.area.border.dashdot.dd.extend(border.dashdot.dd)
                el.area.border.dashdot.offset = border.dashdot.offset
            if border.pathsym._is_set():
                el.area.border.pathsym.name = border.pathsym.name
                el.area.border.pathsym.step = border.pathsym.step
                el.area.border.pathsym.offset = border.pathsym.offset
    if kind & KIND_SYMBOL:
        el.symbol.name = strings[r.varuint()]
        el.symbol.apply_for_type = r.varint()
        el.symbol.priority = r.varint()
        el.symbol.min_distance = r.varint()
    if kind & KIND_CAPTION:
        _read_caption(r, el.caption, strings)
    if kind & KIND_PATHTEXT:
        _read_caption(r, el.path_text, strings)
    if kind & KIND_SHIELD:
        el.shield.height = r.varint()
        el.shield.color = r.varuint()
        el.shield.stroke_color = r.varuint()
        el.shield.priority = r.varint()
        el.shield.min_distance = r.varint()
        el.shield.text_color = r.varuint()
        el.shield.text_stroke_color = r.varuint()
    return el


def _read_line(r, strings, with_priority):
    ln = LineRule()
    flags = r.u8()
    ln.width = r.f32()
    ln.color = r.varuint()
    if with_priority:
        ln.priority = r.varint()
    ln.join = r.u8()
    ln.cap = r.u8()
    if flags & LINE_FLAG_DASHDOT:
        ln.dashdot.dd.extend(r.f32() for _ in range(r.varuint()))
        ln.dashdot.offset = r.f32()
    if flags & LINE_FLAG_PATHSYM:
        ln.pathsym.name = strings[r.varuint()]
        ln.pathsym.step = r.f32()
        ln.pathsym.offset = r.f32()
    return ln


def _read_caption(r, cap, strings):
    flags = r.u8()
    if flags & CAPTION_FLAG_PRIMARY:
        _read_captiondef(r, cap.primary, strings)
    if flags & CAPTION_FLAG_SECONDARY:
        _read_captiondef(r, cap.secondary, strings)
    cap.priority = r.varint()


def _read_captiondef(r, cd, strings):
    cd.height = r.varint()
    cd.color = r.varuint()
    cd.stroke_color = r.varuint()
    cd.offset_x = r.varint()
    cd.offset_y = r.varint()
    cd.text = strings[r.varuint()]
    cd.is_optional = r.u8() != 0


def _resolve(types, named, strings, palette):
    """Rebuilds a Container for one variant, replacing palette indices with this variant's
    color values."""
    def color(idx):
        return palette[idx]

    container = Container()
    for name, color_idx in named:
        ce = ColorElement()
        ce.name = strings[name]
        ce.color = color(color_idx)
        container.colors.value.append(ce)

    for type_name, elements in types:
        ce = ClassifElement()
        ce.name = type_name
        for src in elements:
            ce.element.append(_resolve_element(src, color))
        container.cont.append(ce)
    return container


def _resolve_element(src, color):
    el = copy.deepcopy(src)
    for ln in el.lines:
        ln.color = color(ln.color)
    if el.area._is_set():
        el.area.color = color(el.area.color)
        if el.area.border._is_set():
            el.area.border.color = color(el.area.border.color)
    for cap in (el.caption, el.path_text):
        if cap._is_set():
            for sub in (cap.primary, cap.secondary):
                if sub._is_set():
                    sub.color = color(sub.color)
                    sub.stroke_color = color(sub.stroke_color)
    if el.shield._is_set():
        sh = el.shield
        sh.color = color(sh.color)
        sh.stroke_color = color(sh.stroke_color)
        sh.text_color = color(sh.text_color)
        sh.text_stroke_color = color(sh.text_stroke_color)
    return el


# ------------------------------- canonical text dump -------------------------------

def _g(v):
    return f"{v:.6g}"


def serialize_text(containers, variant_names):
    """Produces the canonical, deterministic, review-only text dump (all variants side by side).
    Colors are written by value, not as palette indices, so that adding or removing a color changes
    only the lines that use it instead of renumbering the whole dump."""
    template, palette, named = _build_indexed(containers)
    out = []
    out.append("# drules text dump, format 2. Generated by generate_drules.sh - do not edit.")
    out.append("# Colors are #TTRRGGBB, where TT is 255 - alpha (00 is opaque), as stored in the rules.")
    out.append("# A color is written once, or once per variant separated by '/' when the variants differ.")
    out.append("variants: " + " ".join(variant_names))

    # One string per palette entry, a single value when every variant agrees.
    colors = [f"#{e[0]:08X}" if len(set(e)) == 1 else "/".join(f"#{v:08X}" for v in e) for e in palette]

    if named:
        out.append("named-colors:")
        for name, color_idx in named:
            out.append(f"  {name} {colors[color_idx]}")

    cap = {ROUNDCAP: "round", BUTTCAP: "butt", SQUARECAP: "square"}
    join = {ROUNDJOIN: "round", BEVELJOIN: "bevel", NOJOIN: "no"}

    def is_transparent(color_idx):  # a halo color that is zero in every variant == "none"
        return all(v == 0 for v in palette[color_idx])

    def line_str(ln, with_priority):
        parts = [f"width={_g(ln.width)}", f"color={colors[ln.color]}",
                 f"join={join[ln.join]}", f"cap={cap[ln.cap]}"]
        if ln.dashdot.dd:
            parts.append("dash=[" + ",".join(_g(d) for d in ln.dashdot.dd) + "]")
            if ln.dashdot.offset:
                parts.append(f"dash_offset={_g(ln.dashdot.offset)}")
        if ln.pathsym._is_set():
            parts.append(f"pathsym={ln.pathsym.name} step={_g(ln.pathsym.step)} offset={_g(ln.pathsym.offset)}")
        if with_priority:
            parts.append(f"priority={ln.priority}")
        return " ".join(parts)

    def captiondef_str(cd):
        parts = [f"height={cd.height}", f"color={colors[cd.color]}"]
        if not is_transparent(cd.stroke_color):
            parts.append(f"stroke={colors[cd.stroke_color]}")
        if cd.offset_x:
            parts.append(f"dx={cd.offset_x}")
        if cd.offset_y:
            parts.append(f"dy={cd.offset_y}")
        if cd.text:
            parts.append(f"text={cd.text!r}")
        if cd.is_optional:
            parts.append("optional")
        return " ".join(parts)

    def caption_str(keyword, cap_rule):
        parts = [keyword]
        if cap_rule.primary._is_set():
            parts.append("primary[" + captiondef_str(cap_rule.primary) + "]")
        if cap_rule.secondary._is_set():
            parts.append("secondary[" + captiondef_str(cap_rule.secondary) + "]")
        parts.append(f"priority={cap_rule.priority}")
        return " ".join(parts)

    for ce in template.cont:
        out.append(f"type {ce.name}")
        for el in ce.element:
            head = f"  z{el.scale}"
            if el.apply_if:
                head += " if " + " and ".join(f'"{a}"' for a in el.apply_if)
            out.append(head)
            for ln in el.lines:
                out.append("    line " + line_str(ln, with_priority=True))
            if el.area._is_set():
                parts = [f"color={colors[el.area.color]}"]
                if el.area.border._is_set():
                    parts.append("border[" + line_str(el.area.border, with_priority=False) + "]")
                parts.append(f"priority={el.area.priority}")
                out.append("    area " + " ".join(parts))
            if el.symbol._is_set():
                parts = [f"name={el.symbol.name}", f"priority={el.symbol.priority}"]
                if el.symbol.apply_for_type:
                    parts.append(f"apply_for_type={el.symbol.apply_for_type}")
                if el.symbol.min_distance:
                    parts.append(f"min_distance={el.symbol.min_distance}")
                out.append("    symbol " + " ".join(parts))
            if el.caption._is_set():
                out.append("    " + caption_str("caption", el.caption))
            if el.path_text._is_set():
                out.append("    " + caption_str("path_text", el.path_text))
            if el.shield._is_set():
                sh = el.shield
                parts = [f"height={sh.height}", f"color={colors[sh.color]}"]
                # Unlike a halo, the plate outline is drawn whenever it differs from the plate color.
                if sh.stroke_color != sh.color:
                    parts.append(f"stroke={colors[sh.stroke_color]}")
                parts.append(f"text_color={colors[sh.text_color]}")
                if not is_transparent(sh.text_stroke_color):
                    parts.append(f"text_stroke={colors[sh.text_stroke_color]}")
                parts.append(f"priority={sh.priority}")
                if sh.min_distance:
                    parts.append(f"min_distance={sh.min_distance}")
                out.append("    shield " + " ".join(parts))

    return "\n".join(out) + "\n"


# ------------------------------- file helpers -------------------------------

def load_all(path):
    with open(path, "rb") as f:
        return parse_binary(f.read())


def load_container(path):
    """Loads a native file and returns its first variant's container (handy for single-variant
    files and for tools that only inspect the shared structure)."""
    return load_all(path)[1][0]


def save_binary(path, containers, variant_names):
    with open(path, "wb") as f:
        f.write(serialize_binary(containers, variant_names))


def save_text(path, containers, variant_names):
    with open(path, "w", encoding="utf-8") as f:
        f.write(serialize_text(containers, variant_names))
