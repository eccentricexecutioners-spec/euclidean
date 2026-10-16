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
