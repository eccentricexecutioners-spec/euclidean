//! LightBurn project files (`.lbrn2`, Euclidean E4 Part C): one sheet (an artboard) written as a
//! LightBurn project in millimetres, one cut layer per stroke colour, every path a LightBurn
//! `Path` shape. LightBurn's format is not published; what is written here follows LightBurn-saved
//! files and the MIT-licensed readers and writers listed in `docs/job-package.md` (nothing copied):
//!
//! - the root `<LightBurnProject AppVersion FormatVersion="1" MaterialHeight MirrorX MirrorY>`, then
//!   one `<CutSetting type="Cut">` per layer used (`<index>`, `<name>`, power and speed left at
//!   LightBurn's defaults, `<priority>`), then the shapes;
//! - a layer's colour is not stored: LightBurn takes it from the layer index (C00 black, C01 blue,
//!   C02 red…, [`PALETTE`]), so each document colour goes to the nearest free index;
//! - `<Shape Type="Path" CutIndex VertID PrimID>` with an identity `<XForm>`, `<VertList>` (`V<x> <y>`
//!   then the handles `c0x c0y` leaving the vertex and `c1x c1y` arriving at it, absolute, or
//!   `c0x1` / `c1x1` for none) and `<PrimList>` (`L<i> <j>` lines, `B<i> <j>` cubics);
//! - millimetres with Y up from the sheet's bottom-left corner (checked against a LightBurn-saved
//!   file's own thumbnail), so a sheet `W` × `H` mm spans 0…W, 0…H.

use std::fmt::Write as _;

use kurbo::PathEl;
use vectorcraft_color::Paint;
use vectorcraft_doc::{Document, Node, NodeKind, Unit};
use vectorcraft_geom::{BezPath, Point, Rect};

/// LightBurn's 30 cut-layer colours, C00…C29 (its documentation's Layer Colors page).
pub const PALETTE: [[u8; 3]; 30] = [
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0xFF],
    [0xFF, 0x00, 0x00],
    [0x00, 0xE0, 0x00],
    [0xD0, 0xD0, 0x00],
    [0xFF, 0x80, 0x00],
    [0x00, 0xE0, 0xE0],
    [0xFF, 0x00, 0xFF],
    [0xB4, 0xB4, 0xB4],
    [0x00, 0x00, 0xA0],
    [0xA0, 0x00, 0x00],
    [0x00, 0xA0, 0x00],
    [0xA0, 0xA0, 0x00],
    [0xC0, 0x80, 0x00],
    [0x00, 0xA0, 0xFF],
    [0xA0, 0x00, 0xA0],
    [0x80, 0x80, 0x80],
    [0x7D, 0x87, 0xB9],
    [0xBB, 0x77, 0x84],
    [0x4A, 0x6F, 0xE3],
    [0xD3, 0x3F, 0x6A],
    [0x8C, 0xD7, 0x8C],
    [0xF0, 0xB9, 0x8D],
    [0xF6, 0xC4, 0xE1],
    [0xFA, 0x9E, 0xD4],
    [0x50, 0x0A, 0x78],
    [0xB4, 0x5A, 0x00],
    [0x00, 0x47, 0x54],
    [0x86, 0xFA, 0x88],
    [0xFF, 0xDB, 0x66],
];

/// The `AppVersion` the file claims: an early LightBurn 1.x number, so any LightBurn reads it as a
/// file from an older version (a non-numeric one might upset its version check). Who wrote the file
/// is in its Notes.
const LB_APP_VERSION: &str = "1.0.00";

/// Most shapes one sheet writes (a guard against runaway documents).
const MAX_SHAPES: usize = 200_000;

/// What an operation does at the laser, by how its art is painted: stroked only → cut; filled →
/// engrave. (Score is for the shop to choose in LightBurn; nothing here picks it.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cut,
    Engrave,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Cut => "cut",
            Kind::Engrave => "engrave",
        }
    }
}

/// A document colour and the LightBurn layer it went to.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub rgb: [u8; 3],
    pub index: usize,
    pub kind: Kind,
    /// How many shapes the layer holds.
    pub shapes: usize,
}

impl Layer {
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.rgb[0], self.rgb[1], self.rgb[2])
    }
}

/// A written sheet: the file's text, its layers, and what was left out.
#[derive(Clone, Debug, Default)]
pub struct Written {
    pub xml: String,
    pub layers: Vec<Layer>,
    pub warnings: Vec<String>,
}

/// Squared distance between two colours.
fn dist(a: [u8; 3], b: [u8; 3]) -> i32 {
    (0..3).map(|i| (i32::from(a[i]) - i32::from(b[i])).pow(2)).sum()
}

/// The layer index for `rgb`: the nearest palette colour no other document colour has taken, so
/// two colours never share a layer (`taken` lists the indices in use). None when all 30 are used.
pub fn nearest_free(rgb: [u8; 3], taken: &[usize]) -> Option<usize> {
    (0..PALETTE.len()).filter(|i| !taken.contains(i)).min_by_key(|i| (dist(rgb, PALETTE[*i]), *i))
}

/// One colour for a paint (a gradient's first stop), or None for no paint.
fn rgb_of(p: &Paint) -> Option<[u8; 3]> {
    let rgb = |c: &vectorcraft_color::Color| {
        let [r, g, b, _] = c.to_rgba8(1.0);
        [r, g, b]
    };
    match p {
        Paint::None => None,
        Paint::Solid { color, .. } => Some(rgb(color)),
        Paint::Gradient(g) => g.gradient.stops.first().map(|s| rgb(&s.color)),
        Paint::Pattern { .. } => Some([0, 0, 0]),
    }
}

/// A number as LightBurn writes them: plain decimals, at most 4 places, no `-0`.
fn num(v: f64) -> String {
    let v = if v.abs() < 5e-5 { 0.0 } else { v };
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

fn escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            c if (c as u32) < 0x20 => " ".into(),
            c => c.to_string(),
        })
        .collect()
}

/// The shapes being collected: per path, its layer slot (index into `colours`) and its geometry in
/// document points.
#[derive(Default)]
struct Walk {
    colours: Vec<([u8; 3], Kind)>,
    paths: Vec<(usize, BezPath)>,
    warnings: Vec<String>,
}

impl Walk {
    fn warn(&mut self, w: &str) {
        if !self.warnings.iter().any(|x| x == w) {
            self.warnings.push(w.to_string());
        }
    }

    fn slot(&mut self, rgb: [u8; 3], kind: Kind) -> usize {
        match self.colours.iter().position(|c| c.0 == rgb) {
            Some(i) => i,
            None => {
                self.colours.push((rgb, kind));
                self.colours.len() - 1
            }
        }
    }

    fn node(&mut self, n: &Node, depth: usize) {
        if !n.visible || depth > 64 || self.paths.len() >= MAX_SHAPES {
            return;
        }
        let shape = |w: &mut Walk, bp: BezPath| {
            let stroke = rgb_of(&n.appearance.stroke_paint());
            let fill = rgb_of(&n.appearance.fill_paint());
            let (rgb, kind) = match (stroke, fill) {
                (Some(s), _) => (s, Kind::Cut),
                (None, Some(f)) => (f, Kind::Engrave),
                // Unpainted art is still a line to the laser: black, cut.
                (None, None) => ([0, 0, 0], Kind::Cut),
            };
            let slot = w.slot(rgb, kind);
            w.paths.push((slot, bp));
        };
        match &n.kind {
            NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } => {
                for c in children {
                    self.node(c, depth + 1);
                }
            }
            NodeKind::Path { path, guide, .. } => {
                if !*guide {
                    shape(self, path.to_bezpath());
                }
            }
            NodeKind::Compound { children, .. } => {
                let mut bp = BezPath::new();
                for p in children.iter().filter_map(|c| c.path_data()) {
                    bp.extend(p.to_bezpath());
                }
                shape(self, bp);
            }
            NodeKind::Text(_) => self.warn("type is left out: outline it first (Type › Create Outlines)"),
            NodeKind::Image(_) => self.warn("placed images are left out"),
            _ => self.warn("symbols, blends, envelopes, meshes, repeats and placed documents are left out: expand them first"),
        }
    }
}

/// A LightBurn vertex: its place, the handle leaving it (c0) and the handle arriving at it (c1),
/// in sheet millimetres.
struct Vert {
    p: (f64, f64),
    c0: Option<(f64, f64)>,
    c1: Option<(f64, f64)>,
}

impl Vert {
    fn at(p: (f64, f64)) -> Self {
        Vert { p, c0: None, c1: None }
    }
}

/// One subpath being built: its vertices and primitives (`true` = a cubic) from vertex to vertex.
#[derive(Default)]
struct Sub {
    verts: Vec<Vert>,
    prims: Vec<(bool, usize, usize)>,
}

impl Sub {
    /// The vertex the next segment starts from (`from` when the subpath is empty).
    fn start_at(&mut self, from: (f64, f64)) {
        if self.verts.is_empty() {
            self.verts.push(Vert::at(from));
        }
    }

    fn line(&mut self, from: (f64, f64), p: (f64, f64)) {
        self.start_at(from);
        self.verts.push(Vert::at(p));
        let n = self.verts.len();
        self.prims.push((false, n - 2, n - 1));
    }

    fn cubic(&mut self, from: (f64, f64), a: (f64, f64), b: (f64, f64), p: (f64, f64)) {
        self.start_at(from);
        if let Some(v) = self.verts.last_mut() {
            v.c0 = Some(a);
        }
        self.verts.push(Vert { p, c0: None, c1: Some(b) });
        let n = self.verts.len();
        self.prims.push((true, n - 2, n - 1));
    }

    /// Close back to vertex 0: a last segment that already ends on it is pointed at vertex 0 (its
    /// arriving handle moves there); otherwise a closing line is added.
    fn close(&mut self, ended_on_start: bool) {
        let n = self.verts.len();
        if n < 2 {
            return;
        }
        if ended_on_start {
            if let Some(last) = self.verts.pop() {
                if let Some(p) = self.prims.last_mut() {
                    p.2 = 0;
                }
                if let Some(v0) = self.verts.first_mut() {
                    v0.c1 = last.c1;
                }
            }
        } else {
            self.prims.push((false, n - 1, 0));
        }
    }

    /// The VertList and PrimList texts; None for a subpath with no segment.
    fn lists(&self) -> Option<(String, String)> {
        if self.prims.is_empty() {
            return None;
        }
        let mut vl = String::new();
        for v in &self.verts {
            let _ = write!(vl, "V{} {}", num(v.p.0), num(v.p.1));
            match v.c0 {
                Some(c) => {
                    let _ = write!(vl, "c0x{}c0y{}", num(c.0), num(c.1));
                }
                None => vl.push_str("c0x1"),
            }
            match v.c1 {
                Some(c) => {
                    let _ = write!(vl, "c1x{}c1y{}", num(c.0), num(c.1));
                }
                None => vl.push_str("c1x1"),
            }
        }
        let mut pl = String::new();
        for (curve, i, j) in &self.prims {
            let _ = write!(pl, "{}{i} {j}", if *curve { "B" } else { "L" });
        }
        Some((vl, pl))
    }
}

/// `bp`'s subpaths as LightBurn (VertList, PrimList) pairs; `to` maps a document point to sheet
/// millimetres (Y up from the sheet's bottom edge).
fn path_lists(bp: &BezPath, to: &dyn Fn(Point) -> (f64, f64)) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut sub = Sub::default();
    let (mut last, mut start) = (Point::ZERO, Point::ZERO);
    for el in bp.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                out.extend(std::mem::take(&mut sub).lists());
                sub.verts.push(Vert::at(to(p)));
                (last, start) = (p, p);
            }
            PathEl::LineTo(p) => {
                sub.line(to(last), to(p));
                last = p;
            }
            PathEl::QuadTo(q, p) => {
                let (a, b) = (last + (q - last) * (2.0 / 3.0), p + (q - p) * (2.0 / 3.0));
                sub.cubic(to(last), to(a), to(b), to(p));
                last = p;
            }
            PathEl::CurveTo(a, b, p) => {
                sub.cubic(to(last), to(a), to(b), to(p));
                last = p;
            }
            PathEl::ClosePath => {
                sub.close((last - start).hypot() < 1e-9);
                out.extend(std::mem::take(&mut sub).lists());
                last = start;
            }
        }
    }
    out.extend(sub.lists());
    out
}

/// Write artboard `index` of `doc` as a LightBurn project: the art wholly on it (as Sheet & Nest
/// counts it), each path in the layer of its stroke colour (its fill's when it has no stroke).
/// `writer` ("Euclidean 0.1.5") goes in the file's Notes.
pub fn write_sheet(doc: &Document, index: usize, writer: &str) -> Result<Written, String> {
    let board = doc.artboards.get(index).ok_or_else(|| format!("no artboard {}", index + 1))?;
    let rect: Rect = board.rect;
    let pt = Unit::Millimeters.to_pt(1.0);
    let (w_mm, h_mm) = (rect.width() / pt, rect.height() / pt);
    let mut walk = Walk::default();
    for id in doc.art_on_artboard(rect, false) {
        if let Some(n) = doc.node(id) {
            walk.node(n, 0);
        }
    }
    if walk.paths.len() >= MAX_SHAPES {
        walk.warn("too many shapes: the sheet was cut short");
    }
    // Layers: colours in first-seen order, each to the nearest free LightBurn index.
    let mut layers: Vec<Layer> = vec![];
    let mut slot_index = vec![];
    for (rgb, kind) in &walk.colours {
        let taken: Vec<usize> = layers.iter().map(|l| l.index).collect();
        let Some(i) = nearest_free(*rgb, &taken) else {
            return Err("more than 30 colours: LightBurn has 30 cut layers".into());
        };
        slot_index.push(i);
        layers.push(Layer { rgb: *rgb, index: i, kind: *kind, shapes: 0 });
    }
    let to = |p: Point| ((p.x - rect.x0) / pt, (rect.y1 - p.y) / pt);
    let mut shapes = String::new();
    let mut id = 0usize;
    for (slot, bp) in &walk.paths {
        let (Some(&cut), Some(layer)) = (slot_index.get(*slot), layers.get_mut(*slot)) else { continue };
        for (vl, pl) in path_lists(bp, &to) {
            let _ = write!(
                shapes,
                "    <Shape Type=\"Path\" CutIndex=\"{cut}\" VertID=\"{id}\" PrimID=\"{id}\">\n        <XForm>1 0 0 1 0 0</XForm>\n        <VertList>{vl}</VertList>\n        <PrimList>{pl}</PrimList>\n    </Shape>\n"
            );
            id += 1;
            layer.shapes += 1;
        }
    }
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        xml,
        "<LightBurnProject AppVersion=\"{LB_APP_VERSION}\" FormatVersion=\"1\" MaterialHeight=\"0\" MirrorX=\"False\" MirrorY=\"False\">"
    );
    xml.push_str("    <VariableText>\n        <Start Value=\"0\"/>\n        <End Value=\"999\"/>\n        <Current Value=\"0\"/>\n        <Increment Value=\"1\"/>\n        <AutoAdvance Value=\"0\"/>\n    </VariableText>\n");
    let mut by_index: Vec<&Layer> = layers.iter().collect();
    by_index.sort_by_key(|l| l.index);
    for (priority, l) in by_index.iter().enumerate() {
        let name = format!("C{:02} {} {}", l.index, l.kind.id(), l.hex());
        // Power and speed are set at the laser: LightBurn's defaults for a new layer.
        let _ = write!(
            xml,
            "    <CutSetting type=\"Cut\">\n        <index Value=\"{}\"/>\n        <name Value=\"{}\"/>\n        <minPower Value=\"10\"/>\n        <maxPower Value=\"20\"/>\n        <maxPower2 Value=\"20\"/>\n        <speed Value=\"100\"/>\n        <priority Value=\"{priority}\"/>\n        <tabCount Value=\"1\"/>\n        <tabCountMax Value=\"1\"/>\n    </CutSetting>\n",
            l.index,
            escape(&name),
        );
    }
    xml.push_str(&shapes);
    let _ = writeln!(
        xml,
        "    <Notes ShowOnLoad=\"0\" Notes=\"{}\"/>",
        escape(&format!("{} · {} × {} mm · written by {writer}", board.name, num(w_mm), num(h_mm)))
    );
    xml.push_str("</LightBurnProject>\n");
    Ok(Written { xml, layers, warnings: walk.warnings })
}
