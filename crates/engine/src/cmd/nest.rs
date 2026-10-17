//! Sheet & Nest (Euclidean, E1 Part F2): lay the selected parts out on sheets of a chosen size by
//! their bounding boxes (shelf packing, first fit, 0° or 90°), as one artboard per sheet with a
//! copy of each part on it drawn as a cut path (no fill, a hairline black stroke). The sheets are
//! then exported one by one with `document.export {format: svg|dxf, artboard}` at true size.
//!
//! A sheet is an artboard with sheet settings (E4 Part B): material, size, thickness, margin,
//! spacing and whether parts may turn, kept in `document.unknown["euclidean.sheets"]` by artboard
//! id, so a 0.1.4 document opens unchanged and 0.1.4 opens (and keeps) the key. Changing a sheet's
//! settings resizes its artboard (top-left fixed) and re-nests the parts on it; the ones that no
//! longer fit go to a new sheet. An artboard 0.1.4's `nest.apply` made ("Sheet N" holding cut paths
//! only) is read as a sheet with the default margin and spacing until it is changed.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId, Unit};
use vectorcraft_geom::{Affine, Rect};

const C_PLAN: &str = "nest.plan";
const C_APPLY: &str = "nest.apply";
const C_ADD: &str = "nest.addSheet";
const C_UPDATE: &str = "nest.updateSheet";
const C_SHEET: &str = "nest.sheet";
/// Where the sheets' settings live in `document.unknown`.
pub const SHEETS_KEY: &str = "euclidean.sheets";
/// Most parts one nest lays out.
const MAX_PARTS: usize = 2000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "nest.plan",
            "Plan Nest",
            [],
            None,
            "{sheet: {width, height} (mm), margin?: 5 (mm, from the sheet's edge), spacing?: 3 (mm, between parts), rotate?: true (a part may turn 90°), ids?: [object ids] (default: the selection)} lays the parts' bounding boxes on as many sheets as they need, biggest first, in rows → {sheets: [{placements: [{id, x, y, width, height, rotated}]}] (mm from the sheet's top-left), unplaced: [ids that fit no sheet]}",
            has_doc,
            plan
        ),
        cmd!(
            "nest.apply",
            "Nest on Sheets",
            [],
            None,
            "{…nest.plan's params, name?: \"Sheet\", material?, thickness? (mm), onto?: artboard index (a sheet: it is filled first, its parts re-nested with the new ones, in its settings)} one artboard per sheet, right of the last artboard and of all the art, with a copy of each part on its place as a cut path (no fill, 0.25 pt black stroke); each artboard made is a sheet with these settings; the copies are selected; one undo step → {artboards: [index], copies: [ids], unplaced}",
            has_doc,
            apply
        ),
        cmd!(
            query "nest.sheet",
            "Sheet Settings",
            [],
            None,
            "{artboard: index} the sheet settings of that artboard → {sheet: {material, width, height, thickness, margin, spacing, rotate} (mm) | null (not a sheet), stored: bool (false: a 0.1.4 nest's \"Sheet N\" read with the default margin and spacing), parts: [ids of the art wholly on it]}",
            has_doc,
            sheet_query
        ),
        cmd!(
            query "nest.sheets",
            "List Sheets",
            [],
            None,
            "{} every artboard that is a sheet → {sheets: [{artboard: index, name, sheet, stored, parts}]}",
            has_doc,
            sheets_query
        ),
        cmd!(
            "nest.addSheet",
            "Add Sheet",
            [],
            None,
            "{settings: {material?, width, height, thickness?, margin?: 5, spacing?: 3, rotate?: true} (mm), name?} an empty sheet: an artboard of that size right of the last artboard and of all the art, named \"Sheet N\". One undo step → {artboard: index}",
            has_doc,
            add_sheet
        ),
        cmd!(
            "nest.updateSheet",
            "Update Sheet",
            [],
            None,
            "{artboard: index, settings: {material?, width?, height?, thickness?, margin?, spacing?, rotate?} (mm; left out: as the sheet has them)} stores the settings, resizes the artboard to the size (top-left fixed) and re-nests the parts on it inside the new margin; parts that no longer fit go to new sheets to the right; one undo step → {artboard, artboards: [index of every sheet used, this one first], moved: n, overflow: n (parts put on new sheets), unplaced: [ids too big for the sheet, left where they were]}",
            has_doc,
            update_sheet
        ),
    ]
}

// ---- sheets (E4 Part B): an artboard with sheet settings ----

/// A sheet's settings (mm), as stored and as the commands take them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetSpec {
    /// The material's name ("Plywood", "Acrylic"); empty when not given.
    #[serde(default)]
    pub material: String,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub thickness: f64,
    #[serde(default = "default_margin")]
    pub margin: f64,
    #[serde(default = "default_spacing")]
    pub spacing: f64,
    #[serde(default = "default_rotate")]
    pub rotate: bool,
}

fn default_margin() -> f64 {
    5.0
}
fn default_spacing() -> f64 {
    3.0
}
fn default_rotate() -> bool {
    true
}

impl SheetSpec {
    fn packing(&self) -> Sheet {
        Sheet { width: self.width, height: self.height, margin: self.margin, spacing: self.spacing, rotate: self.rotate }
    }
}

/// What `document.unknown["euclidean.sheets"]` holds: settings by artboard id.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Sheets {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    sheets: std::collections::BTreeMap<String, SheetSpec>,
}

fn read_sheets(doc: &Document) -> Sheets {
    doc.unknown.get(SHEETS_KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn write_sheets(doc: &mut Document, mut sheets: Sheets) {
    // Settings of artboards that are gone are let go.
    let ids: std::collections::BTreeSet<String> = doc.artboards.iter().map(|a| a.id.to_string()).collect();
    sheets.sheets.retain(|k, _| ids.contains(k));
    sheets.version = 1;
    if sheets.sheets.is_empty() {
        doc.unknown.remove(SHEETS_KEY);
    } else if let Ok(v) = serde_json::to_value(&sheets) {
        doc.unknown.insert(SHEETS_KEY.to_string(), v);
    }
}

/// Forget the sheet settings of the artboards with `ids` (they were deleted).
pub fn forget_sheets(doc: &mut Document, ids: &[u32]) {
    if doc.unknown.contains_key(SHEETS_KEY) {
        let mut sheets = read_sheets(doc);
        for id in ids {
            sheets.sheets.remove(&id.to_string());
        }
        write_sheets(doc, sheets);
    }
}

/// Is artboard `index` a sheet 0.1.4's `nest.apply` made? Named "Sheet N", and holding art that is
/// all cut paths (no fill, a stroke).
fn made_by_nest(doc: &Document, index: usize) -> bool {
    let Some(a) = doc.artboards.get(index) else { return false };
    let numbered = a.name.strip_prefix("Sheet ").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if !numbered {
        return false;
    }
    let art = doc.art_on_artboard(a.rect, false);
    !art.is_empty()
        && art.iter().all(|id| doc.node(*id).is_some_and(|n| n.appearance.fill_paint().is_none() && !n.appearance.stroke_paint().is_none()))
}

/// The sheet settings of artboard `index` and whether they are stored (false: a 0.1.4 nest's
/// sheet, read with the default margin and spacing); None when it is not a sheet.
pub fn sheet_of(doc: &Document, index: usize) -> Option<(SheetSpec, bool)> {
    let a = doc.artboards.get(index)?;
    // In mm to a millionth: points to mm and back leaves 150.00000000000003.
    let pt = Unit::Millimeters.to_pt(1.0);
    let mm = |v: f64| (v / pt * 1e6).round() / 1e6;
    if let Some(mut spec) = read_sheets(doc).sheets.get(&a.id.to_string()).cloned() {
        // The artboard is the sheet: its size wins over what was stored.
        (spec.width, spec.height) = (mm(a.rect.width()), mm(a.rect.height()));
        return Some((spec, true));
    }
    made_by_nest(doc, index).then(|| {
        let spec = SheetSpec {
            material: String::new(),
            width: mm(a.rect.width()),
            height: mm(a.rect.height()),
            thickness: 0.0,
            margin: default_margin(),
            spacing: default_spacing(),
            rotate: true,
        };
        (spec, false)
    })
}

/// Settings from `p` (a `settings` object), over `base` where a key is left out.
fn spec_from(p: &Value, base: Option<&SheetSpec>, cmd: &str) -> Result<SheetSpec> {
    let base = base.cloned().unwrap_or(SheetSpec {
        material: String::new(),
        width: 0.0,
        height: 0.0,
        thickness: 0.0,
        margin: default_margin(),
        spacing: default_spacing(),
        rotate: true,
    });
    let spec = SheetSpec {
        material: match p.get("material") {
            None | Some(Value::Null) => base.material,
            Some(Value::String(m)) => m.trim().chars().take(80).collect(),
            Some(_) => return Err(bad(cmd, "`material` must be text")),
        },
        width: mm(p, "width", base.width, cmd)?,
        height: mm(p, "height", base.height, cmd)?,
        thickness: mm(p, "thickness", base.thickness, cmd)?,
        margin: mm(p, "margin", base.margin, cmd)?,
        spacing: mm(p, "spacing", base.spacing, cmd)?,
        rotate: p.get("rotate").and_then(Value::as_bool).unwrap_or(base.rotate),
    };
    if spec.width <= 2.0 * spec.margin || spec.height <= 2.0 * spec.margin {
        return Err(bad(cmd, "the sheet must be larger than twice its margin"));
    }
    if spec.width > 20_000.0 || spec.height > 20_000.0 {
        return Err(bad(cmd, "a sheet is at most 20 m a side"));
    }
    Ok(spec)
}

fn store_spec(doc: &mut Document, index: usize, spec: &SheetSpec) {
    let Some(id) = doc.artboards.get(index).map(|a| a.id) else { return };
    let mut sheets = read_sheets(doc);
    sheets.sheets.insert(id.to_string(), spec.clone());
    write_sheets(doc, sheets);
}

fn index_param(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let n = s.doc()?.doc.artboards.len();
    p.get("artboard")
        .and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .filter(|i| *i < n)
        .ok_or_else(|| bad(cmd, format!("`artboard` must be an artboard index (0…{})", n.saturating_sub(1))))
}

fn sheet_json(doc: &Document, index: usize) -> Value {
    let parts: Vec<u64> = doc.artboards.get(index).map(|a| doc.art_on_artboard(a.rect, false)).unwrap_or_default().iter().map(|id| id.0).collect();
    match sheet_of(doc, index) {
        Some((spec, stored)) => json!({"artboard": index, "name": doc.artboards[index].name, "sheet": spec, "stored": stored, "parts": parts}),
        None => json!({"artboard": index, "name": doc.artboards.get(index).map(|a| a.name.clone()), "sheet": null, "stored": false, "parts": parts}),
    }
}

fn sheet_query(s: &mut Session, p: &Value) -> Result<Value> {
    let i = index_param(s, p, C_SHEET)?;
    Ok(sheet_json(&s.doc()?.doc, i))
}

fn sheets_query(s: &mut Session, _: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let sheets: Vec<Value> = (0..d.artboards.len()).filter(|i| sheet_of(d, *i).is_some()).map(|i| sheet_json(d, i)).collect();
    Ok(json!({"sheets": sheets}))
}

/// Where the next new sheet goes: right of the last artboard and of all the art, at the last
/// artboard's top.
fn next_place(d: &Document) -> (f64, f64) {
    let last = d.artboards.last().map(|a| a.rect);
    let right = d.artboards.iter().map(|a| a.rect.x1).chain(d.art_bounds().map(|r| r.x1)).fold(f64::MIN, f64::max);
    (if right > f64::MIN { right + 20.0 } else { 0.0 }, last.map_or(0.0, |r| r.y0))
}

/// The next free "Sheet N" name.
fn next_name(d: &Document, stem: &str) -> String {
    let used = |k: usize| d.artboards.iter().any(|a| a.name == format!("{stem} {k}"));
    let k = (1..).find(|k| !used(*k)).unwrap_or(1);
    format!("{stem} {k}")
}

/// A new empty sheet artboard with `spec`, right of everything → its index.
fn new_sheet(s: &mut Session, spec: &SheetSpec, name: Option<&str>, stem: &str, cmd: &str) -> Result<usize> {
    let pt = Unit::Millimeters.to_pt(1.0);
    let (x, y, name) = {
        let d = &s.doc()?.doc;
        let (x, y) = next_place(d);
        (x, y, name.map_or_else(|| next_name(d, stem), str::to_string))
    };
    let r = s.execute("artboard.new", &json!({"x": x, "y": y, "width": spec.width * pt, "height": spec.height * pt, "name": name}))?;
    let index = r["index"].as_u64().ok_or_else(|| bad(cmd, "artboard.new gave no index"))? as usize;
    s.edit("Sheet Settings", |d, _| {
        store_spec(d, index, spec);
        Ok(())
    })?;
    Ok(index)
}

fn add_sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let spec = spec_from(p.get("settings").unwrap_or(&Value::Null), None, C_ADD)?;
    s.begin_named_undo_group(Some("Add Sheet".into()));
    let made = new_sheet(s, &spec, str_param(p, "name"), "Sheet", C_ADD);
    s.end_undo_group(made.is_err());
    Ok(json!({"artboard": made?}))
}

/// Move (and turn, when planned) object `id` so its box sits at `pl` on `board`.
fn put(s: &mut Session, id: NodeId, board: Rect, pl: &Placement, cmd: &str) -> Result<()> {
    let pt = Unit::Millimeters.to_pt(1.0);
    let b: Rect = s.doc()?.doc.bounds_of(&[id], false).ok_or_else(|| bad(cmd, "a part has no bounds"))?;
    let centre = b.center();
    let mut a = Affine::IDENTITY;
    if pl.rotated {
        a = Affine::translate(centre.to_vec2()) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::translate(-centre.to_vec2());
    }
    let now = a.transform_rect_bbox(b);
    let target = (board.x0 + pl.x * pt, board.y0 + pl.y * pt);
    a = Affine::translate((target.0 - now.x0, target.1 - now.y0)) * a;
    if a != Affine::IDENTITY {
        s.execute("object.transform", &json!({"matrix": a.as_coeffs(), "ids": [id.0]}))?;
    }
    Ok(())
}

/// What a layout did: the sheets used (the first one given, if any, first), the copies made, the
/// parts already on a sheet that moved and went to a new one, and the ids that fit no sheet.
struct LaidOut {
    boards: Vec<usize>,
    copies: Vec<NodeId>,
    moved: usize,
    overflow: usize,
    unplaced: Vec<NodeId>,
}

/// Lay `existing` (parts already on a sheet: moved) and copies of `new` (cut-path copies made) out
/// on `first` (a sheet, filled first) and then on new sheets with `spec`. Runs inside an undo group.
fn lay_out(s: &mut Session, spec: &SheetSpec, first: Option<usize>, existing: &[NodeId], new: &[NodeId], stem: &str, cmd: &str) -> Result<LaidOut> {
    let pt = Unit::Millimeters.to_pt(1.0);
    let mut parts = vec![];
    for id in existing.iter().chain(new) {
        let r = s.doc()?.doc.bounds_of(&[*id], false).ok_or_else(|| bad(cmd, format!("object {} has no bounds", id.0)))?;
        parts.push(Part { id: *id, width: r.width() / pt, height: r.height() / pt });
    }
    let (sheets, unplaced) = pack(&parts, &spec.packing());
    let mut out = LaidOut { boards: vec![], copies: vec![], moved: 0, overflow: 0, unplaced };
    for (k, placements) in sheets.iter().enumerate() {
        let index = match (k, first) {
            (0, Some(i)) => i,
            _ => new_sheet(s, spec, None, stem, cmd)?,
        };
        let board = s.doc()?.doc.artboards.get(index).map(|a| a.rect).ok_or_else(|| bad(cmd, "no such artboard"))?;
        out.boards.push(index);
        for pl in placements {
            if existing.contains(&pl.id) {
                put(s, pl.id, board, pl, cmd)?;
                out.moved += 1;
                if first.is_some() && k > 0 {
                    out.overflow += 1;
                }
                continue;
            }
            s.execute("select.set", &json!({"ids": [pl.id.0]}))?;
            let r = s.execute("edit.duplicate", &json!({}))?;
            let copy = r["ids"].get(0).and_then(Value::as_u64).map(NodeId).ok_or_else(|| bad(cmd, "duplicate made nothing"))?;
            put(s, copy, board, pl, cmd)?;
            s.execute("paint.setFill", &json!({"none": true, "ids": [copy.0]}))?;
            s.execute("paint.setStroke", &json!({"color": "#000000", "ids": [copy.0]}))?;
            s.execute("stroke.set", &json!({"weight": 0.25}))?;
            out.copies.push(copy);
        }
    }
    Ok(out)
}

fn update_sheet(s: &mut Session, p: &Value) -> Result<Value> {
    let index = index_param(s, p, C_UPDATE)?;
    let (was, old_rect, parts) = {
        let d = &s.doc()?.doc;
        let a = &d.artboards[index];
        (sheet_of(d, index).map(|x| x.0), a.rect, d.art_on_artboard(a.rect, false))
    };
    let spec = spec_from(p.get("settings").unwrap_or(&Value::Null), was.as_ref(), C_UPDATE)?;
    let pt = Unit::Millimeters.to_pt(1.0);
    s.begin_named_undo_group(Some("Update Sheet".into()));
    let made = (|| -> Result<LaidOut> {
        s.execute(
            "artboard.setProps",
            &json!({"index": index, "x": old_rect.x0, "y": old_rect.y0, "width": spec.width * pt, "height": spec.height * pt}),
        )?;
        s.edit("Sheet Settings", |d, _| {
            store_spec(d, index, &spec);
            Ok(())
        })?;
        let stem = s.doc()?.doc.artboards[index].name.rsplit_once(' ').map_or("Sheet", |(a, _)| a).to_string();
        let out = lay_out(s, &spec, Some(index), &parts, &[], &stem, C_UPDATE)?;
        s.execute("select.set", &json!({"ids": parts.iter().filter(|id| !out.unplaced.contains(id)).map(|id| id.0).collect::<Vec<_>>()}))?;
        Ok(out)
    })();
    s.end_undo_group(made.is_err());
    let out = made?;
    let mut boards = out.boards;
    if boards.first() != Some(&index) {
        boards.insert(0, index);
    }
    Ok(
        json!({"artboard": index, "artboards": boards, "moved": out.moved, "overflow": out.overflow, "unplaced": out.unplaced.iter().map(|id| id.0).collect::<Vec<_>>()}),
    )
}

/// What the packer lays out: a part's bounding box in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub id: NodeId,
    pub width: f64,
    pub height: f64,
}

/// Where a part went: its box's top-left on the sheet (mm from the sheet's top-left corner, the
/// margin included), its size as placed, and whether it was turned 90°.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub id: NodeId,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rotated: bool,
}

/// The nest's settings (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheet {
    pub width: f64,
    pub height: f64,
    pub margin: f64,
    pub spacing: f64,
    pub rotate: bool,
}

/// Shelf packing, first fit decreasing: parts sorted by height (their longer side horizontal
/// when they may turn), rows filled left to right, a part that does not fit the row turned if
/// that helps, else a new row, else a new sheet. A part larger than the sheet's usable area
/// either way is left unplaced.
pub fn pack(parts: &[Part], sheet: &Sheet) -> (Vec<Vec<Placement>>, Vec<NodeId>) {
    let usable_w = sheet.width - 2.0 * sheet.margin;
    let usable_h = sheet.height - 2.0 * sheet.margin;
    let mut unplaced = vec![];
    let mut queue: Vec<(Part, bool)> = vec![];
    for p in parts {
        // Longer side horizontal keeps the rows low.
        let turned = sheet.rotate && p.height > p.width;
        let (w, h) = if turned { (p.height, p.width) } else { (p.width, p.height) };
        if (w <= usable_w + 1e-9 && h <= usable_h + 1e-9) || (sheet.rotate && h <= usable_w + 1e-9 && w <= usable_h + 1e-9) {
            queue.push((Part { id: p.id, width: w, height: h }, turned));
        } else {
            unplaced.push(p.id);
        }
    }
    queue.sort_by(|a, b| {
        b.0.height
            .partial_cmp(&a.0.height)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.0.width.partial_cmp(&a.0.width).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut sheets: Vec<Vec<Placement>> = vec![];
    // The open sheet: rows so far, the current row's top, height and the next free x.
    let (mut row_y, mut row_h, mut x) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut open = false;
    for (p, turned) in queue {
        let fits_here = |w: f64, h: f64, x: f64, row_y: f64, row_h: f64| -> bool {
            let x0 = if x > 0.0 { x + sheet.spacing } else { 0.0 };
            x0 + w <= usable_w + 1e-9 && row_y + h.max(row_h) <= usable_h + 1e-9
        };
        let place = |sheets: &mut Vec<Vec<Placement>>, w: f64, h: f64, rotated: bool, row_y: &mut f64, row_h: &mut f64, x: &mut f64| {
            let x0 = if *x > 0.0 { *x + sheet.spacing } else { 0.0 };
            if let Some(last) = sheets.last_mut() {
                last.push(Placement { id: p.id, x: sheet.margin + x0, y: sheet.margin + *row_y, width: w, height: h, rotated });
            }
            *x = x0 + w;
            *row_h = row_h.max(h);
        };
        let (w, h) = (p.width, p.height);
        loop {
            if !open {
                sheets.push(vec![]);
                (row_y, row_h, x) = (0.0, 0.0, 0.0);
                open = true;
            }
            if fits_here(w, h, x, row_y, row_h) {
                place(&mut sheets, w, h, turned, &mut row_y, &mut row_h, &mut x);
                break;
            }
            if sheet.rotate && fits_here(h, w, x, row_y, row_h) {
                place(&mut sheets, h, w, !turned, &mut row_y, &mut row_h, &mut x);
                break;
            }
            // A new row.
            if row_h > 0.0 {
                let next_y = row_y + row_h + sheet.spacing;
                if next_y + h <= usable_h + 1e-9 || (sheet.rotate && next_y + w <= usable_h + 1e-9) {
                    (row_y, row_h, x) = (next_y, 0.0, 0.0);
                    continue;
                }
            }
            // A new sheet.
            open = false;
        }
    }
    (sheets, unplaced)
}

fn mm(p: &Value, key: &str, default: f64, cmd: &str) -> Result<f64> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| bad(cmd, format!("`{key}` must be a length in mm"))),
    }
}

fn settings(p: &Value, cmd: &str) -> Result<Sheet> {
    let sheet = p.get("sheet").ok_or_else(|| bad(cmd, "missing `sheet`: {width, height} in mm"))?;
    let width = mm(sheet, "width", 0.0, cmd)?;
    let height = mm(sheet, "height", 0.0, cmd)?;
    let margin = mm(p, "margin", 5.0, cmd)?;
    let spacing = mm(p, "spacing", 3.0, cmd)?;
    if width <= 2.0 * margin || height <= 2.0 * margin {
        return Err(bad(cmd, "the sheet must be larger than twice its margin"));
    }
    Ok(Sheet { width, height, margin, spacing, rotate: p.get("rotate").and_then(Value::as_bool).unwrap_or(true) })
}

/// The parts: the given ids or the selected top-level objects, with their geometric bounds in mm.
fn parts(s: &Session, p: &Value, cmd: &str) -> Result<Vec<Part>> {
    let st = s.doc()?;
    let ids: Vec<NodeId> = match p.get("ids").and_then(Value::as_array) {
        Some(a) => a.iter().map(|v| v.as_u64().map(NodeId).ok_or_else(|| bad(cmd, "`ids` must be object ids"))).collect::<Result<_>>()?,
        None => st.selection.objects.clone(),
    };
    if ids.is_empty() {
        return Err(bad(cmd, "select the parts to nest, or pass `ids`"));
    }
    if ids.len() > MAX_PARTS {
        return Err(bad(cmd, format!("at most {MAX_PARTS} parts")));
    }
    let pt = Unit::Millimeters.to_pt(1.0);
    ids.iter()
        .map(|id| {
            let r = st.doc.bounds_of(&[*id], false).ok_or_else(|| bad(cmd, format!("object {} has no bounds", id.0)))?;
            Ok(Part { id: *id, width: r.width() / pt, height: r.height() / pt })
        })
        .collect()
}

fn plan_json(sheets: &[Vec<Placement>], unplaced: &[NodeId]) -> Value {
    json!({
        "sheets": sheets.iter().map(|s| json!({"placements": s.iter().map(|p| json!({"id": p.id.0, "x": p.x, "y": p.y, "width": p.width, "height": p.height, "rotated": p.rotated})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "unplaced": unplaced.iter().map(|id| id.0).collect::<Vec<_>>(),
    })
}

fn plan(s: &mut Session, p: &Value) -> Result<Value> {
    let sheet = settings(p, C_PLAN)?;
    let parts = parts(s, p, C_PLAN)?;
    let (sheets, unplaced) = pack(&parts, &sheet);
    Ok(plan_json(&sheets, &unplaced))
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    let sheet = settings(p, C_APPLY)?;
    let parts = parts(s, p, C_APPLY)?;
    let ids: Vec<NodeId> = parts.iter().map(|p| p.id).collect();
    let onto = match p.get("onto") {
        None | Some(Value::Null) => None,
        Some(_) => Some(index_param(s, &json!({"artboard": p["onto"]}), C_APPLY)?),
    };
    // Onto a sheet: its settings, and the parts already on it are laid out again with the new ones.
    let (spec, existing) = match onto {
        Some(i) => {
            let d = &s.doc()?.doc;
            let spec = sheet_of(d, i).map(|x| x.0).ok_or_else(|| bad(C_APPLY, "`onto` is not a sheet"))?;
            let on: Vec<NodeId> = d.art_on_artboard(d.artboards[i].rect, false).into_iter().filter(|id| !ids.contains(id)).collect();
            (spec, on)
        }
        None => (
            SheetSpec {
                material: str_param(p, "material").unwrap_or_default().trim().chars().take(80).collect(),
                width: sheet.width,
                height: sheet.height,
                thickness: mm(p, "thickness", 0.0, C_APPLY)?,
                margin: sheet.margin,
                spacing: sheet.spacing,
                rotate: sheet.rotate,
            },
            vec![],
        ),
    };
    let all: Vec<Part> = {
        let pt = Unit::Millimeters.to_pt(1.0);
        let d = &s.doc()?.doc;
        existing
            .iter()
            .filter_map(|id| d.bounds_of(&[*id], false).map(|r| Part { id: *id, width: r.width() / pt, height: r.height() / pt }))
            .chain(parts)
            .collect()
    };
    if pack(&all, &spec.packing()).0.is_empty() {
        return Err(bad(C_APPLY, "no part fits the sheet"));
    }
    let stem = str_param(p, "name").unwrap_or("Sheet").to_string();
    s.begin_named_undo_group(Some("Nest on Sheets".into()));
    let made = (|| -> Result<LaidOut> {
        let out = lay_out(s, &spec, onto, &existing, &ids, &stem, C_APPLY)?;
        s.execute("select.set", &json!({"ids": out.copies.iter().map(|c| c.0).collect::<Vec<_>>()}))?;
        Ok(out)
    })();
    s.end_undo_group(made.is_err());
    let out = made?;
    let unplaced: Vec<u64> = out.unplaced.iter().filter(|id| ids.contains(id)).map(|id| id.0).collect();
    Ok(json!({"artboards": out.boards, "copies": out.copies.iter().map(|c| c.0).collect::<Vec<_>>(), "unplaced": unplaced}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(id: u64, w: f64, h: f64) -> Part {
        Part { id: NodeId(id), width: w, height: h }
    }

    fn sheet(w: f64, h: f64) -> Sheet {
        Sheet { width: w, height: h, margin: 5.0, spacing: 5.0, rotate: true }
    }

    /// No part passes the margin; every pair is at least `spacing` apart on one axis.
    fn check(sheets: &[Vec<Placement>], s: &Sheet) {
        for placements in sheets {
            for p in placements {
                assert!(p.x >= s.margin - 1e-9 && p.y >= s.margin - 1e-9, "{p:?}");
                assert!(p.x + p.width <= s.width - s.margin + 1e-9 && p.y + p.height <= s.height - s.margin + 1e-9, "{p:?}");
            }
            for (i, a) in placements.iter().enumerate() {
                for b in &placements[i + 1..] {
                    let dx = (a.x - (b.x + b.width)).max(b.x - (a.x + a.width));
                    let dy = (a.y - (b.y + b.height)).max(b.y - (a.y + a.height));
                    assert!(dx >= s.spacing - 1e-9 || dy >= s.spacing - 1e-9, "{a:?} {b:?} overlap or sit closer than {}", s.spacing);
                }
            }
        }
    }

    #[test]
    fn ten_plates_take_three_sheets_two_by_two() {
        let parts: Vec<Part> = (1..=10).map(|i| part(i, 50.0, 30.0)).collect();
        let s = sheet(150.0, 100.0);
        let (sheets, unplaced) = pack(&parts, &s);
        assert!(unplaced.is_empty());
        // 140 × 90 usable: two flat per row, a third turned on its side at the row's end (x 115, 30 wide,
        // 50 tall), then a second row of two: five a sheet.
        assert_eq!(sheets.iter().map(Vec::len).collect::<Vec<_>>(), [5, 5]);
        check(&sheets, &s);
        assert_eq!((sheets[0][0].x, sheets[0][0].y), (5.0, 5.0));
        assert_eq!((sheets[0][1].x, sheets[0][1].y), (60.0, 5.0), "spacing 5 after a 50 wide part");
        let third = sheets[0][2];
        assert!(third.rotated && (third.x, third.y, third.width, third.height) == (115.0, 5.0, 30.0, 50.0), "{third:?}");
        assert_eq!((sheets[0][3].x, sheets[0][3].y), (5.0, 60.0), "the second row 5 under the tallest of the first");
        // Flat only: two per row, two rows, four a sheet.
        let (sheets, _) = pack(&parts, &Sheet { rotate: false, ..s });
        assert_eq!(sheets.iter().map(Vec::len).collect::<Vec<_>>(), [4, 4, 2]);
    }

    #[test]
    fn a_long_part_turns_to_fit_and_one_too_big_is_left_out() {
        let s = sheet(100.0, 150.0);
        let (sheets, unplaced) = pack(&[part(1, 120.0, 20.0), part(2, 200.0, 20.0), part(3, 30.0, 30.0)], &s);
        assert_eq!(unplaced, [NodeId(2)], "200 fits neither way in 90 × 140");
        assert_eq!(sheets.len(), 1);
        let long = sheets[0].iter().find(|p| p.id == NodeId(1)).unwrap();
        assert!(long.rotated && long.width == 20.0 && long.height == 120.0, "{long:?}");
        check(&sheets, &s);
        // Without rotation it fits nowhere.
        let (sheets, unplaced) = pack(&[part(1, 120.0, 20.0)], &Sheet { rotate: false, ..s });
        assert!(sheets.is_empty() && unplaced == [NodeId(1)]);
    }

    #[test]
    fn mixed_sizes_stay_inside_and_apart() {
        let parts: Vec<Part> = (0..40).map(|i| part(i, 10.0 + (i * 7 % 60) as f64, 8.0 + (i * 11 % 45) as f64)).collect();
        let s = Sheet { width: 300.0, height: 200.0, margin: 8.0, spacing: 4.0, rotate: true };
        let (sheets, unplaced) = pack(&parts, &s);
        assert!(unplaced.is_empty());
        assert_eq!(sheets.iter().map(Vec::len).sum::<usize>(), 40);
        check(&sheets, &s);
    }
}
