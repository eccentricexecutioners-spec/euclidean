//! Sheet & Nest (Euclidean, E1 Part F2): parts laid out on a sheet artboard as cut paths, inside
//! the margin and apart by the spacing, and the sheet exported at true size as SVG and DXF.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;

const MM: f64 = 72.0 / 25.4;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn text(v: &Value) -> String {
    String::from_utf8(vectorcraft_format::base64_decode(v["dataBase64"].as_str().unwrap()).unwrap()).unwrap()
}

/// Two 50 × 30 mm plates, selected, nested on a 150 × 100 mm sheet (margin 5, spacing 3).
fn nested() -> (Session, usize, Vec<NodeId>) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 400, "height": 300}));
    for x in [10.0, 100.0] {
        run(&mut s, "shape.rectangle", json!({"x": x * MM, "y": 10.0 * MM, "width": 50.0 * MM, "height": 30.0 * MM}));
        run(&mut s, "paint.setFill", json!({"color": "#ff0000"}));
    }
    run(&mut s, "select.all", json!({}));
    let r = run(&mut s, "nest.apply", json!({"sheet": {"width": 150, "height": 100}, "margin": 5, "spacing": 3}));
    let boards = r["artboards"].as_array().unwrap();
    assert_eq!(boards.len(), 1, "{r}");
    let copies = r["copies"].as_array().unwrap().iter().map(|v| NodeId(v.as_u64().unwrap())).collect();
    (s, boards[0].as_u64().unwrap() as usize, copies)
}

#[test]
fn two_plates_sit_inside_the_margin_apart_as_cut_paths() {
    let (s, board, copies) = nested();
    let st = s.doc().unwrap();
    let a = st.doc.artboards[board].rect;
    assert!((a.width() - 150.0 * MM).abs() < 1e-6 && (a.height() - 100.0 * MM).abs() < 1e-6, "{a:?}");
    assert_eq!(copies.len(), 2);
    let boxes: Vec<_> = copies.iter().map(|id| st.doc.bounds_of(&[*id], false).unwrap()).collect();
    for b in &boxes {
        assert!(b.x0 >= a.x0 + 5.0 * MM - 1e-6 && b.y0 >= a.y0 + 5.0 * MM - 1e-6, "{b:?} inside the margin of {a:?}");
        assert!(b.x1 <= a.x1 - 5.0 * MM + 1e-6 && b.y1 <= a.y1 - 5.0 * MM + 1e-6, "{b:?} inside the margin of {a:?}");
        assert!((b.width() - 50.0 * MM).abs() < 1e-6 && (b.height() - 30.0 * MM).abs() < 1e-6, "true size: {b:?}");
    }
    let gap = (boxes[1].x0 - boxes[0].x1).max(boxes[0].x0 - boxes[1].x1).max((boxes[1].y0 - boxes[0].y1).max(boxes[0].y0 - boxes[1].y1));
    assert!(gap >= 3.0 * MM - 1e-6, "3 mm apart: {gap}");
    for id in &copies {
        let n = st.doc.node(*id).unwrap();
        assert!(n.appearance.fill_paint().is_none(), "no fill: {:?}", n.appearance.fill_paint());
        assert!(!n.appearance.stroke_paint().is_none(), "a stroke: {:?}", n.appearance.stroke_paint());
    }
    // The originals stay where they were.
    assert_eq!(st.doc.artboards.len(), 2);
}

#[test]
fn nest_is_one_undo_step() {
    let (mut s, _, _) = nested();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc().unwrap().doc.artboards.len(), 1, "the sheet goes with one undo");
}

#[test]
fn the_sheet_exports_at_true_size_as_svg_and_dxf() {
    let (mut s, board, _) = nested();
    let range = (board + 1).to_string();
    let svg = text(&run(&mut s, "document.export", json!({"format": "svg", "useArtboards": true, "range": range, "sizeUnit": "mm"})));
    assert!(svg.contains("width=\"150mm\" height=\"100mm\""), "{}", &svg[..svg.len().min(400)]);
    assert_eq!(svg.matches("<rect").count() + svg.matches("<path").count(), 2, "the two cut paths alone:\n{svg}");
    let plain = text(&run(&mut s, "document.export", json!({"format": "svg", "useArtboards": true, "range": range})));
    assert!(plain.contains("width=\"425.197\""), "bare points stay the default");
    let dxf = text(&run(&mut s, "document.export", json!({"format": "dxf", "useArtboards": true, "range": range})));
    let lines: Vec<&str> = dxf.lines().collect();
    let var = |name: &str| lines.iter().position(|l| *l == name).map(|i| lines[i + 2].trim().to_string());
    assert_eq!(var("$INSUNITS").as_deref(), Some("4"), "millimetres");
    let (x0, y0) = (
        var("$EXTMIN").unwrap().parse::<f64>().unwrap(),
        lines[lines.iter().position(|l| *l == "$EXTMIN").unwrap() + 4].trim().parse::<f64>().unwrap(),
    );
    let i = lines.iter().position(|l| *l == "$EXTMAX").unwrap();
    let (x1, y1) = (lines[i + 2].trim().parse::<f64>().unwrap(), lines[i + 4].trim().parse::<f64>().unwrap());
    // The parts span 5 … 5 + 50 + 3 + 50 mm across and 30 mm up, in millimetres.
    assert!(((x1 - x0) - 103.0).abs() < 0.01 && ((y1 - y0) - 30.0).abs() < 0.01, "extents {x0},{y0} … {x1},{y1}");
}

// ---------------------------------------------------------------- E4 Part B: sheets are artboards

fn sheet(s: &mut Session, board: usize) -> Value {
    run(s, "nest.sheet", json!({"artboard": board}))
}

fn inside_margin(s: &Session, board: usize, ids: &[u64], margin: f64) {
    let st = s.doc().unwrap();
    let a = st.doc.artboards[board].rect;
    for id in ids {
        let b = st.doc.bounds_of(&[NodeId(*id)], false).unwrap();
        assert!(b.x0 >= a.x0 + margin * MM - 1e-6 && b.y0 >= a.y0 + margin * MM - 1e-6, "{b:?} inside the margin of {a:?}");
        assert!(b.x1 <= a.x1 - margin * MM + 1e-6 && b.y1 <= a.y1 - margin * MM + 1e-6, "{b:?} inside the margin of {a:?}");
    }
}

#[test]
fn a_nest_makes_sheets_that_keep_their_settings_and_the_original_artboard_is_not_one() {
    let (mut s, board, copies) = nested();
    let v = sheet(&mut s, board);
    assert_eq!(v["stored"], json!(true), "{v}");
    assert_eq!((v["sheet"]["width"].as_f64(), v["sheet"]["height"].as_f64()), (Some(150.0), Some(100.0)));
    assert_eq!((v["sheet"]["margin"].as_f64(), v["sheet"]["spacing"].as_f64()), (Some(5.0), Some(3.0)));
    let mut on: Vec<u64> = v["parts"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
    on.sort_unstable();
    assert_eq!(on, copies.iter().map(|c| c.0).collect::<Vec<_>>());
    assert_eq!(sheet(&mut s, 0)["sheet"], Value::Null, "the drawing's artboard is not a sheet");
    assert_eq!(run(&mut s, "nest.sheets", json!({}))["sheets"].as_array().unwrap().len(), 1);
}

#[test]
fn changing_a_sheets_width_resizes_that_artboard_only_and_renests_its_parts_inside_the_margin() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 400, "height": 300}));
    for i in 0..3 {
        run(&mut s, "shape.rectangle", json!({"x": (10.0 + 60.0 * i as f64) * MM, "y": 10.0 * MM, "width": 50.0 * MM, "height": 30.0 * MM}));
    }
    run(&mut s, "select.all", json!({}));
    // Three plates in a row on a 200 × 100 sheet.
    let r =
        run(&mut s, "nest.apply", json!({"sheet": {"width": 200, "height": 100}, "margin": 5, "spacing": 3, "material": "Plywood", "thickness": 6}));
    let board = r["artboards"][0].as_u64().unwrap() as usize;
    let copies: Vec<u64> = r["copies"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let others: Vec<_> = s.doc().unwrap().doc.artboards.iter().enumerate().filter(|(i, _)| *i != board).map(|(_, a)| a.rect).collect();
    let top_left = s.doc().unwrap().doc.artboards[board].rect.origin();
    assert_eq!(sheet(&mut s, board)["sheet"]["material"], "Plywood");
    // 130 wide, margin 8: the third plate no longer fits the row and goes below.
    let r = run(&mut s, "nest.updateSheet", json!({"artboard": board, "settings": {"width": 130, "margin": 8}}));
    assert_eq!((r["overflow"].as_u64(), r["unplaced"].as_array().map(Vec::len)), (Some(0), Some(0)), "{r}");
    let st = s.doc().unwrap();
    let a = st.doc.artboards[board].rect;
    assert!((a.width() - 130.0 * MM).abs() < 1e-6 && (a.height() - 100.0 * MM).abs() < 1e-6, "{a:?}");
    assert_eq!(a.origin(), top_left, "the top-left stays");
    let after: Vec<_> = st.doc.artboards.iter().enumerate().filter(|(i, _)| *i != board).map(|(_, a)| a.rect).collect();
    assert_eq!(after, others, "no other artboard changed");
    inside_margin(&s, board, &copies, 8.0);
    let v = sheet(&mut s, board);
    assert_eq!(
        (v["sheet"]["margin"].as_f64(), v["sheet"]["material"].as_str(), v["sheet"]["thickness"].as_f64()),
        (Some(8.0), Some("Plywood"), Some(6.0)),
        "{v}"
    );
    assert_eq!(v["parts"].as_array().unwrap().len(), 3);
    // One undo puts the sheet and its parts back as they were.
    run(&mut s, "edit.undo", json!({}));
    let a = s.doc().unwrap().doc.artboards[board].rect;
    assert!((a.width() - 200.0 * MM).abs() < 1e-6, "{a:?}");
    assert_eq!(sheet(&mut s, board)["sheet"]["margin"].as_f64(), Some(5.0));
}

#[test]
fn parts_that_no_longer_fit_go_to_a_new_sheet_to_the_right_and_add_sheet_makes_an_empty_one() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 400, "height": 300}));
    for i in 0..4 {
        run(&mut s, "shape.rectangle", json!({"x": (10.0 + 60.0 * i as f64) * MM, "y": 10.0 * MM, "width": 50.0 * MM, "height": 30.0 * MM}));
    }
    run(&mut s, "select.all", json!({}));
    let r = run(&mut s, "nest.apply", json!({"sheet": {"width": 120, "height": 80}, "margin": 5, "spacing": 3}));
    assert_eq!(r["artboards"].as_array().unwrap().len(), 1, "two by two on 120 × 80: {r}");
    let board = r["artboards"][0].as_u64().unwrap() as usize;
    let n = s.doc().unwrap().doc.artboards.len();
    // 60 × 80: one plate a row, two a sheet: two go to a new sheet.
    let r = run(&mut s, "nest.updateSheet", json!({"artboard": board, "settings": {"width": 60}}));
    assert_eq!(r["overflow"].as_u64(), Some(2), "{r}");
    let boards: Vec<usize> = r["artboards"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
    assert_eq!((boards.len(), boards[0]), (2, board), "{r}");
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.artboards.len(), n + 1);
    let (a, b) = (d.artboards[board].rect, d.artboards[boards[1]].rect);
    assert!(b.x0 > a.x1, "the new sheet is to the right: {a:?} {b:?}");
    assert!((b.width() - 60.0 * MM).abs() < 1e-6, "with the same settings: {b:?}");
    assert_eq!(d.art_on_artboard(a, false).len() + d.art_on_artboard(b, false).len(), 4);
    let second = sheet(&mut s, boards[1]);
    assert_eq!((second["stored"].clone(), second["sheet"]["width"].as_f64()), (json!(true), Some(60.0)));
    // Add Sheet: an empty sheet right of everything, one undo step.
    let r = run(&mut s, "nest.addSheet", json!({"settings": {"material": "Acrylic", "width": 600, "height": 400, "thickness": 3}}));
    let added = r["artboard"].as_u64().unwrap() as usize;
    let v = sheet(&mut s, added);
    assert_eq!((v["sheet"]["material"].as_str(), v["parts"].as_array().map(Vec::len)), (Some("Acrylic"), Some(0)), "{v}");
    assert!(s.doc().unwrap().doc.artboards[added].name.starts_with("Sheet "));
    let x = s.doc().unwrap().doc.artboards[added].rect.x0;
    assert!(x > s.doc().unwrap().doc.artboards[boards[1]].rect.x1);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc().unwrap().doc.artboards.len(), n + 1, "one undo takes the added sheet away");
    // Nest onto a sheet: it is filled first (with what was on it, in its settings), then new
    // sheets as needed. The 60 × 80 sheet holds its two plates; a 20 × 20 square fits beside
    // neither, so it gets a sheet of its own; a 10 × 10 one also goes there.
    for w in [20.0, 10.0] {
        run(&mut s, "shape.rectangle", json!({"x": 10.0 * MM, "y": 200.0 * MM, "width": w * MM, "height": w * MM}));
    }
    let squares: Vec<u64> = {
        let d = &s.doc().unwrap().doc;
        let n = d.layers[0].children().map_or(0, |c| c.len());
        d.layers[0].children().unwrap()[n - 2..].iter().map(|c| c.id.0).collect()
    };
    run(&mut s, "select.set", json!({"ids": squares}));
    let r = run(&mut s, "nest.apply", json!({"sheet": {"width": 1, "height": 1}, "margin": 0, "onto": boards[1]}));
    let used: Vec<usize> = r["artboards"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
    assert_eq!(used[0], boards[1], "the sheet first: {r}");
    assert_eq!(used.len(), 2, "then one new sheet: {r}");
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.art_on_artboard(d.artboards[used[0]].rect, false).len(), 2);
    assert_eq!(d.art_on_artboard(d.artboards[used[1]].rect, false).len(), 2);
    assert!((d.artboards[used[1]].rect.width() - 60.0 * MM).abs() < 1e-6, "in the sheet's settings, not the params'");
}

#[test]
fn a_0_1_4_nest_reads_as_sheets_and_still_exports_at_true_size() {
    // What 0.1.4 wrote: the same artboards and cut paths, no sheet settings.
    let (mut s, board, _) = nested();
    s.edit("forget", |d, _| {
        d.unknown.remove("euclidean.sheets");
        Ok(())
    })
    .unwrap();
    let bytes = vectorcraft_format::save(&s.doc().unwrap().doc, false);
    let mut s = Session::new();
    run(&mut s, "document.open", json!({"dataBase64": vectorcraft_format::base64_encode(&bytes), "name": "nest-0.1.4.vectorcraft"}));
    let v = sheet(&mut s, board);
    assert_eq!((v["stored"].clone(), v["sheet"]["width"].as_f64(), v["sheet"]["margin"].as_f64()), (json!(false), Some(150.0), Some(5.0)), "{v}");
    assert_eq!(sheet(&mut s, 0)["sheet"], Value::Null);
    assert!(!s.doc().unwrap().doc.unknown.contains_key("euclidean.sheets"), "opening changes nothing");
    let svg =
        text(&run(&mut s, "document.export", json!({"format": "svg", "useArtboards": true, "range": (board + 1).to_string(), "sizeUnit": "mm"})));
    assert!(svg.contains("width=\"150mm\" height=\"100mm\""), "{}", &svg[..svg.len().min(400)]);
    // Its first change stores the settings.
    run(&mut s, "nest.updateSheet", json!({"artboard": board, "settings": {"spacing": 4}}));
    assert_eq!(sheet(&mut s, board)["stored"], json!(true));
}
