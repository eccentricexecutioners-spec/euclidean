//! Sheet & Nest (Euclidean, E1 Part F2): lay the selected parts out on sheets of a chosen size by
//! their bounding boxes (shelf packing, first fit, 0° or 90°), as one artboard per sheet with a
//! copy of each part on it drawn as a cut path (no fill, a hairline black stroke). The sheets are
//! then exported one by one with `document.export {format: svg|dxf, artboard}` at true size.
use super::*;
use serde_json::{Value, json};
use vectorcraft_doc::{NodeId, Unit};
use vectorcraft_geom::{Affine, Rect};

const C_PLAN: &str = "nest.plan";
const C_APPLY: &str = "nest.apply";
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
            "{…nest.plan's params, name?: \"Sheet\"} one artboard per sheet, right of the last artboard and of all the art, with a copy of each part on its place as a cut path (no fill, 0.25 pt black stroke); the copies are selected; one undo step → {artboards: [index], copies: [ids], unplaced}",
            has_doc,
            apply
        ),
    ]
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
    let (sheets, unplaced) = pack(&parts, &sheet);
    if sheets.is_empty() {
        return Err(bad(C_APPLY, "no part fits the sheet"));
    }
    let name = str_param(p, "name").unwrap_or("Sheet").to_string();
    let pt = Unit::Millimeters.to_pt(1.0);
    s.begin_named_undo_group(Some("Nest on Sheets".into()));
    let made = (|| -> Result<(Vec<usize>, Vec<NodeId>)> {
        let mut boards = vec![];
        let mut copies = vec![];
        // The sheets start right of the last artboard and of all the art, so no part hanging off
        // an artboard lands on a sheet (and in its export); then each 20 pt right of the last.
        let (mut next_x, y) = {
            let d = &s.doc()?.doc;
            let last = d.artboards.last().map(|a| a.rect);
            let right = last.map(|r| r.x1).into_iter().chain(d.art_bounds().map(|r| r.x1)).fold(f64::MIN, f64::max);
            (if right > f64::MIN { right + 20.0 } else { 0.0 }, last.map_or(0.0, |r| r.y0))
        };
        for (i, placements) in sheets.iter().enumerate() {
            let r = s.execute(
                "artboard.new",
                &json!({"x": next_x, "y": y, "width": sheet.width * pt, "height": sheet.height * pt, "name": format!("{name} {}", i + 1)}),
            )?;
            let index = r["index"].as_u64().ok_or_else(|| bad(C_APPLY, "artboard.new gave no index"))? as usize;
            let board = s.doc()?.doc.artboards.get(index).map(|a| a.rect).ok_or_else(|| bad(C_APPLY, "no such artboard"))?;
            boards.push(index);
            next_x = board.x1 + 20.0;
            for pl in placements {
                s.execute("select.set", &json!({"ids": [pl.id.0]}))?;
                let r = s.execute("edit.duplicate", &json!({}))?;
                let copy = r["ids"].get(0).and_then(Value::as_u64).map(NodeId).ok_or_else(|| bad(C_APPLY, "duplicate made nothing"))?;
                let b: Rect = s.doc()?.doc.bounds_of(&[copy], false).ok_or_else(|| bad(C_APPLY, "the copy has no bounds"))?;
                let centre = b.center();
                let mut a = Affine::IDENTITY;
                if pl.rotated {
                    a = Affine::translate(centre.to_vec2()) * Affine::rotate(std::f64::consts::FRAC_PI_2) * Affine::translate(-centre.to_vec2());
                }
                let now = a.transform_rect_bbox(b);
                let target = (board.x0 + pl.x * pt, board.y0 + pl.y * pt);
                a = Affine::translate((target.0 - now.x0, target.1 - now.y0)) * a;
                let c = a.as_coeffs();
                s.execute("object.transform", &json!({"matrix": c, "ids": [copy.0]}))?;
                s.execute("paint.setFill", &json!({"none": true, "ids": [copy.0]}))?;
                s.execute("paint.setStroke", &json!({"color": "#000000", "ids": [copy.0]}))?;
                s.execute("stroke.set", &json!({"weight": 0.25}))?;
                copies.push(copy);
            }
        }
        s.execute("select.set", &json!({"ids": copies.iter().map(|c| c.0).collect::<Vec<_>>()}))?;
        Ok((boards, copies))
    })();
    s.end_undo_group(made.is_err());
    let (boards, copies) = made?;
    Ok(
        json!({"artboards": boards, "copies": copies.iter().map(|c| c.0).collect::<Vec<_>>(), "unplaced": unplaced.iter().map(|id| id.0).collect::<Vec<_>>()}),
    )
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
