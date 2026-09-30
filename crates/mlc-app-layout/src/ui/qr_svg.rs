//! Reads the QR code the payment processor draws (`pay[].qr_svg`) into a
//! module matrix the renderer paints itself.
//!
//! Why not hand the SVG to `draw_svg_icon`: that path scales by
//! `size / viewBox` with a non-integer factor, which blurs the module edges a
//! phone camera has to find, and it paints in ONE caller-chosen colour. A QR
//! code is a grid of whole modules, so the renderer wants integer module
//! pixels and fixed dark-on-light contrast. The processor's document is the
//! `qrcode` crate's SVG: a `viewBox="0 0 N N"`, a background `<rect>` and one
//! `<path d="M x y h8 v8 H x V y ...">` of module squares. The crate scales
//! every module to 8 view-box units by default (`default_unit_size`), so a
//! 45-module canvas has `viewBox="0 0 360 360"`; the reader divides the
//! view-box by the largest common unit of its coordinates to get back to
//! whole modules. This reader accepts exactly that family — axis-aligned,
//! integer-coordinate rectangles — and refuses anything else (`None`), in
//! which case the pay step shows the address only and says so.
//!
//! The document comes from a server response, so the reader is bounded: a
//! byte cap on the input and a cap on the number of rectangles it will keep.

/// Fill of the dark modules. Scanners need dark-on-light with full contrast,
/// so these two are deliberately NOT theme tokens: a dark theme must not
/// invert the code.
pub const QR_DARK: &str = "#000000";
/// Fill behind the modules and the quiet zone around them. See [`QR_DARK`].
pub const QR_LIGHT: &str = "#ffffff";

/// Largest document the reader will look at.
const MAX_SVG_BYTES: usize = 128 * 1024;
/// Largest number of rectangles the reader will keep (a version-40 code has
/// 177 x 177 modules; merged into row runs it stays far below this).
const MAX_RECTS: usize = 40_000;
/// Largest side, in modules, the reader accepts.
const MAX_SIDE: u32 = 400;
/// Largest `viewBox` side, in document units, the reader looks at (a module
/// may be several units wide: the `qrcode` crate draws 8 units per module).
const MAX_VIEW_BOX: u32 = 8192;

/// A run of dark modules: `w` x `h` modules whose top-left module is `(x, y)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QrRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// The dark modules of one QR code, in module units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrMatrix {
    /// Modules per side, quiet zone included (the document's `viewBox`).
    pub size: u32,
    /// Dark runs, row-major.
    pub dark: Vec<QrRect>,
}

impl QrMatrix {
    /// Read `svg`; `None` when it is not the rectangle-path family described
    /// in the module doc.
    pub fn parse(svg: &str) -> Option<QrMatrix> {
        if svg.len() > MAX_SVG_BYTES {
            return None;
        }
        let view = parse_view_box_side(svg)?;
        let mut rects: Vec<QrRect> = Vec::new();
        let mut rest = svg;
        while let Some(at) = rest.find("<path") {
            let tag_start = at + "<path".len();
            let tag_end = rest[tag_start..].find('>').map(|e| tag_start + e)?;
            let tag = &rest[tag_start..tag_end];
            if let Some(d) = attribute(tag, "d") {
                parse_path(d, view, &mut rects)?;
            }
            rest = &rest[tag_end..];
        }
        if rects.is_empty() {
            return None;
        }
        // Module unit: the largest step every coordinate and the view-box
        // are a whole multiple of (8 for the `qrcode` crate's default).
        let unit = rects
            .iter()
            .flat_map(|r| [r.x, r.y, r.w, r.h])
            .fold(view, gcd);
        if unit == 0 || view / unit > MAX_SIDE {
            return None;
        }
        let size = view / unit;
        for r in &mut rects {
            *r = QrRect { x: r.x / unit, y: r.y / unit, w: r.w / unit, h: r.h / unit };
        }
        rects.sort_by_key(|r| (r.y, r.x));
        let dark = merge_row_runs(rects);
        if dark.len() > MAX_RECTS {
            return None;
        }
        Some(QrMatrix { size, dark })
    }

    /// Module size in whole pixels that fits the code inside a `side` x `side`
    /// square (never below 1).
    pub fn module_px(&self, side: f64) -> f64 {
        (side / f64::from(self.size)).floor().max(1.0)
    }
}

/// Greatest common divisor (`gcd(0, n) == n`).
fn gcd(a: u32, b: u32) -> u32 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The square side of `viewBox="0 0 N N"` in document units, when it is a
/// whole number within bounds.
fn parse_view_box_side(svg: &str) -> Option<u32> {
    let root_start = svg.find("<svg")?;
    let root_end = svg[root_start..].find('>').map(|e| root_start + e)?;
    let root = &svg[root_start..root_end];
    let view_box = attribute(root, "viewBox")?;
    let nums: Vec<f64> = view_box
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<_, _>>()
        .ok()?;
    if nums.len() != 4 || nums[0] != 0.0 || nums[1] != 0.0 || nums[2] != nums[3] {
        return None;
    }
    let side = nums[2];
    if side.fract() != 0.0 || side < 1.0 || side > f64::from(MAX_VIEW_BOX) {
        return None;
    }
    Some(side as u32)
}

/// Value of `name="..."` (or `name='...'`) inside one tag's text, matched on a
/// whole attribute name.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(found) = tag[from..].find(name) {
        let at = from + found;
        let before_ok = at == 0
            || tag[..at]
                .chars()
                .next_back()
                .map_or(true, |c| c.is_whitespace());
        let after = &tag[at + name.len()..];
        let after_trim = after.trim_start();
        if before_ok && after_trim.starts_with('=') {
            let value = after_trim[1..].trim_start();
            let quote = value.chars().next().filter(|q| *q == '"' || *q == '\'')?;
            let body = &value[1..];
            let end = body.find(quote)?;
            return Some(&body[..end]);
        }
        from = at + name.len();
    }
    None
}

/// One token of path data.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Cmd(char),
    Num(f64),
}

fn tokenize(d: &str) -> Option<Vec<Token>> {
    let bytes = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() || c == ',' {
            i += 1;
        } else if c.is_ascii_alphabetic() {
            if c == 'e' || c == 'E' {
                return None;
            }
            out.push(Token::Cmd(c));
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let n = bytes[i] as char;
                if n.is_ascii_digit() || n == '.' {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Token::Num(d[start..i].parse::<f64>().ok()?));
        } else {
            return None;
        }
    }
    Some(out)
}

/// Turn path data into rectangles, one per sub-path. Every point of a
/// sub-path must sit on the integer grid inside `[0, view]` and every move
/// must be horizontal or vertical; the rectangle is the sub-path's bounding
/// box. `None` for a curve, a diagonal, a fraction or a point off the grid.
fn parse_path(d: &str, view: u32, out: &mut Vec<QrRect>) -> Option<()> {
    let tokens = tokenize(d)?;
    let limit = f64::from(view);
    let mut i = 0;
    let (mut cx, mut cy) = (0.0_f64, 0.0_f64);
    let mut bounds: Option<(f64, f64, f64, f64)> = None;
    let mut cmd = ' ';

    let flush = |bounds: &mut Option<(f64, f64, f64, f64)>, out: &mut Vec<QrRect>| -> Option<()> {
        if let Some((x0, y0, x1, y1)) = bounds.take() {
            let (w, h) = (x1 - x0, y1 - y0);
            if w > 0.0 && h > 0.0 {
                out.push(QrRect { x: x0 as u32, y: y0 as u32, w: w as u32, h: h as u32 });
            }
        }
        Some(())
    };
    let visit = |bounds: &mut Option<(f64, f64, f64, f64)>, x: f64, y: f64| -> Option<()> {
        if x.fract() != 0.0 || y.fract() != 0.0 || x < 0.0 || y < 0.0 || x > limit || y > limit {
            return None;
        }
        *bounds = Some(match *bounds {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
        Some(())
    };

    while i < tokens.len() {
        if let Token::Cmd(c) = tokens[i] {
            cmd = c;
            i += 1;
            if matches!(cmd, 'Z' | 'z') {
                flush(&mut bounds, out)?;
                continue;
            }
        }
        // Arguments of the current command; a bare number list repeats it.
        let need = match cmd {
            'M' | 'm' | 'L' | 'l' => 2,
            'H' | 'h' | 'V' | 'v' => 1,
            _ => return None,
        };
        let mut args = [0.0_f64; 2];
        for slot in args.iter_mut().take(need) {
            match tokens.get(i) {
                Some(Token::Num(n)) => {
                    *slot = *n;
                    i += 1;
                }
                _ => return None,
            }
        }
        match cmd {
            'M' | 'm' => {
                flush(&mut bounds, out)?;
                let (nx, ny) = if cmd == 'M' { (args[0], args[1]) } else { (cx + args[0], cy + args[1]) };
                (cx, cy) = (nx, ny);
                visit(&mut bounds, cx, cy)?;
                // Further coordinate pairs after a moveto are linetos.
                cmd = if cmd == 'M' { 'L' } else { 'l' };
            }
            'L' | 'l' => {
                let (nx, ny) = if cmd == 'L' { (args[0], args[1]) } else { (cx + args[0], cy + args[1]) };
                if nx != cx && ny != cy {
                    return None;
                }
                (cx, cy) = (nx, ny);
                visit(&mut bounds, cx, cy)?;
            }
            'H' => {
                cx = args[0];
                visit(&mut bounds, cx, cy)?;
            }
            'h' => {
                cx += args[0];
                visit(&mut bounds, cx, cy)?;
            }
            'V' => {
                cy = args[0];
                visit(&mut bounds, cx, cy)?;
            }
            'v' => {
                cy += args[0];
                visit(&mut bounds, cx, cy)?;
            }
            _ => return None,
        }
        if out.len() > MAX_RECTS {
            return None;
        }
    }
    flush(&mut bounds, out)
}

/// Merge horizontally adjacent single-row runs so a row of dark modules is
/// one rectangle. `rects` must be sorted by `(y, x)`.
fn merge_row_runs(rects: Vec<QrRect>) -> Vec<QrRect> {
    let mut out: Vec<QrRect> = Vec::with_capacity(rects.len());
    for r in rects {
        if let Some(last) = out.last_mut() {
            if last.y == r.y && last.h == r.h && last.x + last.w == r.x {
                last.w += r.w;
                continue;
            }
        }
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The document `qrcode::render::svg::Color` emits (what dig2pay sends):
    /// a 5x5 canvas with a 1-module quiet zone and a plus-shaped 3x3 code.
    const PROCESSOR_SVG: &str = concat!(
        r#"<?xml version="1.0" standalone="yes"?>"#,
        r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="5" height="5" viewBox="0 0 5 5" shape-rendering="crispEdges">"#,
        r##"<rect x="0" y="0" width="5" height="5" fill="#fff"/>"##,
        r##"<path fill="#000" d="M2 1h1v1H2V1M1 2h1v1H1V2M2 2h1v1H2V2M3 2h1v1H3V2M2 3h1v1H2V3"/></svg>"##,
    );

    #[test]
    fn parses_the_processors_document_and_merges_row_runs() {
        let qr = QrMatrix::parse(PROCESSOR_SVG).expect("processor document parses");
        assert_eq!(qr.size, 5);
        assert_eq!(
            qr.dark,
            vec![
                QrRect { x: 2, y: 1, w: 1, h: 1 },
                QrRect { x: 1, y: 2, w: 3, h: 1 },
                QrRect { x: 2, y: 3, w: 1, h: 1 },
            ]
        );
    }

    #[test]
    fn accepts_relative_moves_and_single_quotes() {
        let svg = "<svg viewBox='0 0 4 4'><path d='M1 1h2v2h-2z'/></svg>";
        let qr = QrMatrix::parse(svg).expect("relative path parses");
        assert_eq!(qr.dark, vec![QrRect { x: 1, y: 1, w: 2, h: 2 }]);
    }

    #[test]
    fn refuses_curves_diagonals_and_fractions() {
        assert!(QrMatrix::parse(r#"<svg viewBox="0 0 4 4"><path d="M0 0C1 1 2 2 3 3"/></svg>"#).is_none());
        assert!(QrMatrix::parse(r#"<svg viewBox="0 0 4 4"><path d="M0 0L2 2"/></svg>"#).is_none());
        assert!(QrMatrix::parse(r#"<svg viewBox="0 0 4 4"><path d="M0.5 0h1v1H0.5V0"/></svg>"#).is_none());
    }

    #[test]
    fn refuses_a_document_without_a_usable_view_box_or_paths() {
        assert!(QrMatrix::parse("").is_none());
        assert!(QrMatrix::parse(r#"<svg><path d="M0 0h1v1H0V0"/></svg>"#).is_none());
        assert!(QrMatrix::parse(r#"<svg viewBox="0 0 4 5"><path d="M0 0h1v1H0V0"/></svg>"#).is_none());
        assert!(QrMatrix::parse(r#"<svg viewBox="0 0 1 1"><rect width="1" height="1"/></svg>"#).is_none());
    }

    #[test]
    fn refuses_a_point_outside_the_view_box() {
        assert!(QrMatrix::parse(r#"<svg viewBox="0 0 4 4"><path d="M3 3h2v1H3V3"/></svg>"#).is_none());
    }

    #[test]
    fn scripts_and_foreign_markup_are_never_interpreted() {
        // The mock processor in the server tests ships a <script> here; it is
        // not a rectangle path, so the reader keeps nothing.
        assert!(QrMatrix::parse("<svg><script>alert(1)</script></svg>").is_none());
    }

    /// What `qrcode` 0.14.1 really emits with its defaults (dig2pay-server
    /// `qr::render_svg`): 8 view-box units per module, one `M..h8v8H..V..`
    /// square per dark module. A 3x3 plus-shaped code with the crate's
    /// 4-module quiet zone is an 11-module canvas, `viewBox="0 0 88 88"`.
    fn crate_svg() -> String {
        let mut path = String::new();
        for (mx, my) in [(5u32, 4u32), (4, 5), (5, 5), (6, 5), (5, 6)] {
            let (x, y) = (mx * 8, my * 8);
            path.push_str(&format!("M{x} {y}h8v8H{x}V{y}"));
        }
        format!(
            concat!(
                r#"<?xml version="1.0" standalone="yes"?>"#,
                r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="88" height="88" viewBox="0 0 88 88" shape-rendering="crispEdges">"#,
                r##"<rect x="0" y="0" width="88" height="88" fill="#fff"/>"##,
                r##"<path fill="#000" d="{}"/></svg>"##,
            ),
            path
        )
    }

    #[test]
    fn parses_the_crates_default_eight_units_per_module() {
        let qr = QrMatrix::parse(&crate_svg()).expect("crate document parses");
        assert_eq!(qr.size, 11);
        assert_eq!(
            qr.dark,
            vec![
                QrRect { x: 5, y: 4, w: 1, h: 1 },
                QrRect { x: 4, y: 5, w: 3, h: 1 },
                QrRect { x: 5, y: 6, w: 1, h: 1 },
            ]
        );
    }

    #[test]
    fn parses_a_real_sized_payload_larger_than_400_units() {
        // A ~70-byte payment URI is a version-5 code: (37 + 8) * 8 = 360; a
        // longer one crosses 400 units. Build a full-size grid checkerboard.
        let modules = 57u32; // version 10 (57 modules) + quiet zone 8 = 65 -> 520 units
        let canvas = modules + 8;
        let mut path = String::new();
        for my in 0..modules {
            for mx in 0..modules {
                if (mx + my) % 2 == 0 {
                    let (x, y) = ((mx + 4) * 8, (my + 4) * 8);
                    path.push_str(&format!("M{x} {y}h8v8H{x}V{y}"));
                }
            }
        }
        let side = canvas * 8;
        let svg = format!(
            r##"<svg viewBox="0 0 {side} {side}"><rect width="{side}" height="{side}" fill="#fff"/><path fill="#000" d="{path}"/></svg>"##
        );
        let qr = QrMatrix::parse(&svg).expect("520-unit document parses");
        assert_eq!(qr.size, canvas);
        assert_eq!(qr.dark.first(), Some(&QrRect { x: 4, y: 4, w: 1, h: 1 }));
    }

    #[test]
    fn module_pixels_are_whole_and_at_least_one() {
        let qr = QrMatrix::parse(PROCESSOR_SVG).expect("parses");
        assert_eq!(qr.module_px(136.0), 27.0);
        assert_eq!(qr.module_px(3.0), 1.0);
    }
}
