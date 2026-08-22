//! CSS color parsing ported from the Python reference implementation (plus the
//! `md5` fallback used by `parse_color_rgb`).

use md5::Digest;

pub type CairoColor = (f64, f64, f64);

const CSS3_NAMES_TO_RGB: &[(&str, u8, u8, u8)] = &[
    ("aliceblue", 240, 248, 255),
    ("antiquewhite", 250, 235, 215),
    ("aqua", 0, 255, 255),
    ("aquamarine", 127, 255, 212),
    ("azure", 240, 255, 255),
    ("beige", 245, 245, 220),
    ("bisque", 255, 228, 196),
    ("black", 0, 0, 0),
    ("blanchedalmond", 255, 235, 205),
    ("blue", 0, 0, 255),
    ("blueviolet", 138, 43, 226),
    ("brown", 165, 42, 42),
    ("burlywood", 222, 184, 135),
    ("cadetblue", 95, 158, 160),
    ("chartreuse", 127, 255, 0),
    ("chocolate", 210, 105, 30),
    ("coral", 255, 127, 80),
    ("cornflowerblue", 100, 149, 237),
    ("cornsilk", 255, 248, 220),
    ("crimson", 220, 20, 60),
    ("cyan", 0, 255, 255),
    ("darkblue", 0, 0, 139),
    ("darkcyan", 0, 139, 139),
    ("darkgoldenrod", 184, 134, 11),
    ("darkgray", 169, 169, 169),
    ("darkgrey", 169, 169, 169),
    ("darkgreen", 0, 100, 0),
    ("darkkhaki", 189, 183, 107),
    ("darkmagenta", 139, 0, 139),
    ("darkolivegreen", 85, 107, 47),
    ("darkorange", 255, 140, 0),
    ("darkorchid", 153, 50, 204),
    ("darkred", 139, 0, 0),
    ("darksalmon", 233, 150, 122),
    ("darkseagreen", 143, 188, 143),
    ("darkslateblue", 72, 61, 139),
    ("darkslategray", 47, 79, 79),
    ("darkslategrey", 47, 79, 79),
    ("darkturquoise", 0, 206, 209),
    ("darkviolet", 148, 0, 211),
    ("deeppink", 255, 20, 147),
    ("deepskyblue", 0, 191, 255),
    ("dimgray", 105, 105, 105),
    ("dimgrey", 105, 105, 105),
    ("dodgerblue", 30, 144, 255),
    ("firebrick", 178, 34, 34),
    ("floralwhite", 255, 250, 240),
    ("forestgreen", 34, 139, 34),
    ("fuchsia", 255, 0, 255),
    ("gainsboro", 220, 220, 220),
    ("ghostwhite", 248, 248, 255),
    ("gold", 255, 215, 0),
    ("goldenrod", 218, 165, 32),
    ("gray", 128, 128, 128),
    ("grey", 128, 128, 128),
    ("green", 0, 128, 0),
    ("greenyellow", 173, 255, 47),
    ("honeydew", 240, 255, 240),
    ("hotpink", 255, 105, 180),
    ("indianred", 205, 92, 92),
    ("indigo", 75, 0, 130),
    ("ivory", 255, 255, 240),
    ("khaki", 240, 230, 140),
    ("lavender", 230, 230, 250),
    ("lavenderblush", 255, 240, 245),
    ("lawngreen", 124, 252, 0),
    ("lemonchiffon", 255, 250, 205),
    ("lightblue", 173, 216, 230),
    ("lightcoral", 240, 128, 128),
    ("lightcyan", 224, 255, 255),
    ("lightgoldenrodyellow", 250, 250, 210),
    ("lightgray", 211, 211, 211),
    ("lightgrey", 211, 211, 211),
    ("lightgreen", 144, 238, 144),
    ("lightpink", 255, 182, 193),
    ("lightsalmon", 255, 160, 122),
    ("lightseagreen", 32, 178, 170),
    ("lightskyblue", 135, 206, 250),
    ("lightslategray", 119, 136, 153),
    ("lightslategrey", 119, 136, 153),
    ("lightsteelblue", 176, 196, 222),
    ("lightyellow", 255, 255, 224),
    ("lime", 0, 255, 0),
    ("limegreen", 50, 205, 50),
    ("linen", 250, 240, 230),
    ("magenta", 255, 0, 255),
    ("maroon", 128, 0, 0),
    ("mediumaquamarine", 102, 205, 170),
    ("mediumblue", 0, 0, 205),
    ("mediumorchid", 186, 85, 211),
    ("mediumpurple", 147, 112, 216),
    ("mediumseagreen", 60, 179, 113),
    ("mediumslateblue", 123, 104, 238),
    ("mediumspringgreen", 0, 250, 154),
    ("mediumturquoise", 72, 209, 204),
    ("mediumvioletred", 199, 21, 133),
    ("midnightblue", 25, 25, 112),
    ("mintcream", 245, 255, 250),
    ("mistyrose", 255, 228, 225),
    ("moccasin", 255, 228, 181),
    ("navajowhite", 255, 222, 173),
    ("navy", 0, 0, 128),
    ("oldlace", 253, 245, 230),
    ("olive", 128, 128, 0),
    ("olivedrab", 107, 142, 35),
    ("orange", 255, 165, 0),
    ("orangered", 255, 69, 0),
    ("orchid", 218, 112, 214),
    ("palegoldenrod", 238, 232, 170),
    ("palegreen", 152, 251, 152),
    ("paleturquoise", 175, 238, 238),
    ("palevioletred", 219, 112, 147),
    ("papayawhip", 255, 239, 213),
    ("peachpuff", 255, 218, 185),
    ("peru", 205, 133, 63),
    ("pink", 255, 192, 203),
    ("plum", 221, 160, 221),
    ("powderblue", 176, 224, 230),
    ("purple", 128, 0, 128),
    ("red", 255, 0, 0),
    ("rosybrown", 188, 143, 143),
    ("royalblue", 65, 105, 225),
    ("saddlebrown", 139, 69, 19),
    ("salmon", 250, 128, 114),
    ("sandybrown", 244, 164, 96),
    ("seagreen", 46, 139, 87),
    ("seashell", 255, 245, 238),
    ("sienna", 160, 82, 45),
    ("silver", 192, 192, 192),
    ("skyblue", 135, 206, 235),
    ("slateblue", 106, 90, 205),
    ("slategray", 112, 128, 144),
    ("slategrey", 112, 128, 144),
    ("snow", 255, 250, 250),
    ("springgreen", 0, 255, 127),
    ("steelblue", 70, 130, 180),
    ("tan", 210, 180, 140),
    ("teal", 0, 128, 128),
    ("thistle", 216, 191, 216),
    ("tomato", 255, 99, 71),
    ("turquoise", 64, 224, 208),
    ("violet", 238, 130, 238),
    ("wheat", 245, 222, 179),
    ("white", 255, 255, 255),
    ("whitesmoke", 245, 245, 245),
    ("yellow", 255, 255, 0),
    ("yellowgreen", 154, 205, 50),
];

fn normalize_hex(hex_value: &str) -> Option<String> {
    let digits = hex_value.strip_prefix('#')?;
    let ok = |c: char| c.is_ascii_hexdigit();
    match digits.len() {
        1 if ok(digits.chars().next().unwrap()) => {
            Some(format!("#{}", digits.repeat(6).to_lowercase()))
        }
        3 if digits.chars().all(ok) => Some(format!(
            "#{}",
            digits
                .chars()
                .map(|c| format!("{c}{c}"))
                .collect::<String>()
                .to_lowercase()
        )),
        6 if digits.chars().all(ok) => Some(format!("#{}", digits.to_lowercase())),
        _ => None,
    }
}

pub fn hex_to_rgb(hex_value: &str) -> Option<(f64, f64, f64)> {
    let norm = normalize_hex(hex_value)?;
    let r = u8::from_str_radix(&norm[1..3], 16).ok()?;
    let g = u8::from_str_radix(&norm[3..5], 16).ok()?;
    let b = u8::from_str_radix(&norm[5..7], 16).ok()?;
    Some((r as f64, g as f64, b as f64))
}

pub fn name_to_rgb(name: &str) -> Option<(f64, f64, f64)> {
    let name = name.to_lowercase();
    for &(n, r, g, b) in CSS3_NAMES_TO_RGB {
        if n == name {
            return Some((r as f64, g as f64, b as f64));
        }
    }
    None
}

pub fn rgb_to_hex(rgb: (f64, f64, f64)) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        rgb.0 as i64 as i32, rgb.1 as i64 as i32, rgb.2 as i64 as i32
    )
}

pub fn cairo_to_hex(cairo: CairoColor) -> String {
    rgb_to_hex((cairo.0 * 255.0, cairo.1 * 255.0, cairo.2 * 255.0))
}

fn md5_hex(s: &str) -> String {
    let mut h = md5::Md5::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    let mut s = String::with_capacity(32);
    for b in out {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

pub fn parse_color_rgb(string: &str) -> Option<(f64, f64, f64)> {
    let string = string.trim().to_lowercase();
    if let Some(v) = name_to_rgb(&string) {
        return Some(v);
    }
    if let Some(v) = hex_to_rgb(&string) {
        return Some(v);
    }
    if string.starts_with("rgb") {
        let inner = string.get(4..string.len().saturating_sub(1))?;
        let parts: Vec<f64> = inner
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();
        if parts.len() >= 3 {
            return Some((parts[0], parts[1], parts[2]));
        }
        // rgb() parse failed: md5 fallback, exactly as in the Python original.
        return hex_to_rgb(&format!("#{}", &md5_hex(&string)[..6]));
    }
    None
}

pub fn parse_color_hex(string: &str) -> Option<String> {
    parse_color_rgb(string).map(|rgb| rgb_to_hex(rgb).to_uppercase())
}

pub fn parse_color_cairo(string: &str) -> Option<CairoColor> {
    parse_color_rgb(string).map(|a| (a.0 / 255.0, a.1 / 255.0, a.2 / 255.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_names() {
        assert_eq!(name_to_rgb("navy"), Some((0.0, 0.0, 128.0)));
        assert_eq!(name_to_rgb("CadetBlue"), Some((95.0, 158.0, 160.0)));
        assert_eq!(name_to_rgb("nope"), None);
    }

    #[test]
    fn test_hex() {
        assert_eq!(hex_to_rgb("#000080"), Some((0.0, 0.0, 128.0)));
        assert_eq!(hex_to_rgb("#f00"), Some((255.0, 0.0, 0.0)));
        assert_eq!(hex_to_rgb("#09c"), Some((0.0, 153.0, 204.0)));
        // The vendored webcolors accepts 1-digit hex (`#0` == `#000000`).
        assert_eq!(hex_to_rgb("#0"), Some((0.0, 0.0, 0.0)));
        assert_eq!(hex_to_rgb("#ggg"), None);
    }

    #[test]
    fn test_whatever() {
        assert_eq!(parse_color_hex("white"), Some("#FFFFFF".to_string()));
        assert_eq!(
            parse_color_hex("rgb(0, 153, 204)"),
            Some("#0099CC".to_string())
        );
        assert_eq!(parse_color_cairo("white"), Some((1.0, 1.0, 1.0)));
    }

    #[test]
    fn test_rgb_to_hex_truncation() {
        // rgb_to_hex casts floats to int (truncation), as in Python int().
        assert_eq!(rgb_to_hex((2.999, 0.0, 0.0)), "#020000");
    }
}
