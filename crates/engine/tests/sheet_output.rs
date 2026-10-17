//! What leaves Euclidean for the laser (E4 Parts C and D): a sheet as a LightBurn project read
//! back as XML (millimetres, the sheet's size, one layer per colour, every cut path, true to
//! size), and a job folder whose `job.json` names every file in it and round-trips.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_engine::Session;

const MM: f64 = 72.0 / 25.4;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

// ---- a small, strict XML reader: elements, attributes, text; tags must balance ----

#[derive(Debug, Default)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<El>,
    text: String,
}

impl El {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.0 == k).map(|a| a.1.as_str())
    }
    fn all<'a>(&'a self, name: &str, out: &mut Vec<&'a El>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.all(name, out);
        }
    }
    fn find(&self, name: &str) -> Vec<&El> {
        let mut v = vec![];
        self.all(name, &mut v);
        v
    }
    fn child(&self, name: &str) -> &El {
        self.children.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no <{name}> in <{}>", self.name))
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn parse_xml(src: &str) -> El {
    let src = src.trim_start();
    let body = match src.strip_prefix("<?xml") {
        Some(rest) => &rest[rest.find("?>").expect("prolog ends") + 2..],
        None => src,
    };
    let mut stack: Vec<El> = vec![El { name: "#doc".into(), ..Default::default() }];
    let mut rest = body;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("</") {
            let end = r.find('>').expect("closing tag ends");
            let name = r[..end].trim();
            let el = stack.pop().unwrap();
            assert_eq!(el.name, name, "tags balance");
            stack.last_mut().unwrap().children.push(el);
            rest = &r[end + 1..];
        } else if let Some(r) = rest.strip_prefix('<') {
            let end = r.find('>').expect("tag ends");
            let mut tag = &r[..end];
            let empty = tag.ends_with('/');
            if empty {
                tag = &tag[..tag.len() - 1];
            }
            let (name, mut attrs_src) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
            let mut el = El { name: name.to_string(), ..Default::default() };
            loop {
                attrs_src = attrs_src.trim_start();
                if attrs_src.is_empty() {
                    break;
                }
                let eq = attrs_src.find('=').expect("attr has =");
                let k = attrs_src[..eq].trim().to_string();
                let v_src = attrs_src[eq + 1..].trim_start();
                assert!(v_src.starts_with('"'), "attr {k} is quoted");
                let close = v_src[1..].find('"').expect("attr quote closes") + 1;
                assert!(!el.attrs.iter().any(|a| a.0 == k), "attr {k} once");
                el.attrs.push((k, unescape(&v_src[1..close])));
                attrs_src = &v_src[close + 1..];
            }
            if empty {
                stack.last_mut().unwrap().children.push(el);
            } else {
                stack.push(el);
            }
            rest = &r[end + 1..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            assert!(!rest[..end].contains('&') || rest[..end].contains(';'), "entities are escaped");
            stack.last_mut().unwrap().text.push_str(&unescape(&rest[..end]));
            rest = &rest[end..];
        }
    }
    assert_eq!(stack.len(), 1, "every element closes");
    let mut doc = stack.pop().unwrap();
    assert_eq!(doc.children.len(), 1, "one root");
    doc.children.pop().unwrap()
}

/// `V<x> <y>` vertices of a VertList.
fn verts(list: &str) -> Vec<(f64, f64)> {
    list.split('V')
        .filter(|v| !v.is_empty())
        .map(|v| {
            let end = v.find('c').unwrap_or(v.len());
            let (x, y) = v[..end].split_once(' ').unwrap();
            (x.parse().unwrap(), y.parse().unwrap())
        })
        .collect()
}

/// A 200 × 120 mm sheet holding a red-stroked 50 × 30 plate and a blue-stroked 20 mm circle,
/// both nested (black cut paths), then recoloured: the plates red, the circle blue.
fn two_colour_sheet() -> (Session, usize, u64, u64) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 400, "height": 300}));
    run(&mut s, "shape.rectangle", json!({"x": 10.0 * MM, "y": 10.0 * MM, "width": 50.0 * MM, "height": 30.0 * MM}));
    run(&mut s, "shape.ellipse", json!({"x": 100.0 * MM, "y": 10.0 * MM, "width": 20.0 * MM, "height": 20.0 * MM}));
    run(&mut s, "select.all", json!({}));
    let r = run(&mut s, "nest.apply", json!({"sheet": {"width": 200, "height": 120}, "margin": 5, "spacing": 3, "rotate": false}));
    let board = r["artboards"][0].as_u64().unwrap() as usize;
    let copies: Vec<u64> = r["copies"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    // Biggest first: the plate, then the circle.
    let (plate, circle) = (copies[0], copies[1]);
    run(&mut s, "paint.setStroke", json!({"color": "#ff0000", "ids": [plate]}));
    run(&mut s, "paint.setStroke", json!({"color": "#0000ff", "ids": [circle]}));
    (s, board, plate, circle)
}

#[test]
fn a_sheet_written_for_lightburn_reads_back_in_mm_at_true_size_with_one_layer_per_colour() {
    let (mut s, board, ..) = two_colour_sheet();
    let r = run(&mut s, "nest.exportLightBurn", json!({"artboard": board}));
    let text = r["files"][0]["text"].as_str().unwrap();
    let root = parse_xml(text);
    assert_eq!(root.name, "LightBurnProject");
    assert_eq!(root.attr("FormatVersion"), Some("1"));
    assert_eq!((root.attr("MirrorX"), root.attr("MirrorY")), (Some("False"), Some("False")));
    // One layer per colour: red → C02, blue → C01 (LightBurn's own red and blue).
    let cuts = root.find("CutSetting");
    let idx: Vec<&str> = cuts.iter().map(|c| c.child("index").attr("Value").unwrap()).collect();
    assert_eq!(idx, ["1", "2"], "{text}");
    for c in &cuts {
        assert_eq!(c.attr("type"), Some("Cut"), "Line mode");
    }
    let layers = r["layers"].as_array().unwrap();
    let pairs: Vec<(String, u64)> = layers.iter().map(|l| (l["colour"].as_str().unwrap().to_string(), l["index"].as_u64().unwrap())).collect();
    assert!(pairs.contains(&("#ff0000".into(), 2)) && pairs.contains(&("#0000ff".into(), 1)), "{pairs:?}");
    // Every cut path is there, on its colour's layer.
    let shapes = root.find("Shape");
    assert_eq!(shapes.len(), 2, "{text}");
    let by_layer = |k: &str| shapes.iter().find(|sh| sh.attr("CutIndex") == Some(k)).unwrap();
    // The plate: 50 × 30 mm, 5 mm in from the sheet's left and top: y up from the bottom edge,
    // so it spans x 5…55 and y 120 − 35 … 120 − 5.
    let plate = verts(&by_layer("2").child("VertList").text);
    let (xs, ys): (Vec<f64>, Vec<f64>) = plate.iter().copied().unzip();
    let span = |v: &[f64]| (v.iter().copied().fold(f64::MAX, f64::min), v.iter().copied().fold(f64::MIN, f64::max));
    let ((x0, x1), (y0, y1)) = (span(&xs), span(&ys));
    assert!((x0 - 5.0).abs() < 1e-3 && (x1 - 55.0).abs() < 1e-3, "x {x0}…{x1}");
    assert!((y0 - 85.0).abs() < 1e-3 && (y1 - 115.0).abs() < 1e-3, "y {y0}…{y1} (mm, y up)");
    assert!(by_layer("2").child("PrimList").text.contains("L"), "the plate is lines");
    // The circle: 20 mm across, as cubics.
    let circle = by_layer("1");
    assert!(circle.child("PrimList").text.contains('B'), "the circle is curves");
    let (cx, _): (Vec<f64>, Vec<f64>) = verts(&circle.child("VertList").text).into_iter().unzip();
    let (c0, c1) = span(&cx);
    assert!(((c1 - c0) - 20.0).abs() < 1e-3, "20 mm across: {c0}…{c1}");
    // Inside the sheet's 200 × 120 mm.
    for sh in &shapes {
        for (x, y) in verts(&sh.child("VertList").text) {
            assert!((0.0..=200.0).contains(&x) && (0.0..=120.0).contains(&y), "({x}, {y}) on the sheet");
        }
    }
}

#[test]
fn colours_go_to_the_nearest_free_layer_and_two_never_share_one() {
    use vectorcraft_engine::cmd::fileio::lbrn::nearest_free;
    assert_eq!(nearest_free([0, 0, 0], &[]), Some(0));
    assert_eq!(nearest_free([250, 5, 5], &[]), Some(2), "near red");
    assert_eq!(nearest_free([0, 0, 0], &[0]), Some(27), "black taken: the next nearest, C27 #004754");
    let taken: Vec<usize> = (0..30).collect();
    assert_eq!(nearest_free([1, 2, 3], &taken), None, "30 layers at most");
}

#[test]
fn a_two_sheet_job_folder_holds_every_file_job_json_names_and_job_json_round_trips() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 400, "height": 300}));
    for i in 0..4 {
        run(&mut s, "shape.rectangle", json!({"x": (10.0 + 60.0 * i as f64) * MM, "y": 10.0 * MM, "width": 50.0 * MM, "height": 30.0 * MM}));
    }
    run(&mut s, "select.all", json!({}));
    let r =
        run(&mut s, "nest.apply", json!({"sheet": {"width": 120, "height": 50}, "margin": 5, "spacing": 3, "material": "Plywood", "thickness": 6}));
    assert_eq!(r["artboards"].as_array().unwrap().len(), 2, "two plates a sheet: {r}");
    let dir = std::env::temp_dir().join(format!("euclidean-job-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let r = run(
        &mut s,
        "nest.exportJob",
        json!({"path": dir.to_string_lossy(), "id": "00000000-0000-4000-8000-000000000000", "created": "2026-10-17T18:30:00Z"}),
    );
    let job: Value = serde_json::from_slice(&std::fs::read(dir.join("job.json")).unwrap()).unwrap();
    assert_eq!(job, r["job"], "what was written is what the command says");
    assert_eq!(job["schema"], "euclidean-job/1");
    assert_eq!(job["order"], json!({"account": null, "link_code": null, "shop": null}), "empty until ordering fills it");
    let sheets = job["sheets"].as_array().unwrap();
    assert_eq!(sheets.len(), 2);
    for sh in sheets {
        assert_eq!((sh["material"].as_str(), sh["thickness_mm"].as_f64()), (Some("Plywood"), Some(6.0)), "{sh}");
        assert_eq!((sh["width_mm"].as_f64(), sh["height_mm"].as_f64(), sh["part_count"].as_u64()), (Some(120.0), Some(50.0), Some(2)), "{sh}");
        let files: Vec<&str> = sh["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        assert_eq!(files.iter().filter(|f| f.ends_with(".svg") || f.ends_with(".dxf") || f.ends_with(".lbrn2")).count(), 3, "{files:?}");
        for f in files.iter().chain([&sh["thumbnail"].as_str().unwrap()]) {
            let bytes = std::fs::read(dir.join(f)).unwrap_or_else(|e| panic!("{f} named in job.json is in the folder: {e}"));
            assert!(!bytes.is_empty(), "{f}");
        }
        let svg = std::fs::read_to_string(dir.join(files.iter().find(|f| f.ends_with(".svg")).unwrap())).unwrap();
        assert!(svg.contains("width=\"120mm\" height=\"50mm\""), "true size");
        let png = std::fs::read(dir.join(sh["thumbnail"].as_str().unwrap())).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
    let ops = job["operations"].as_array().unwrap();
    assert_eq!(ops, &vec![json!({"colour": "#000000", "kind": "cut", "lightburn_layer": 0})]);
    // Round trip: parse → serialise → parse gives the same job.
    let again: Value = serde_json::from_str(&serde_json::to_string(&job).unwrap()).unwrap();
    assert_eq!(again, job);
    // Every file in the folder is one job.json names (and job.json itself).
    let mut named: Vec<String> = sheets
        .iter()
        .flat_map(|sh| {
            sh["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).chain([sh["thumbnail"].as_str().unwrap().to_string()])
        })
        .collect();
    named.push("job.json".into());
    named.sort();
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    on_disk.sort();
    assert_eq!(on_disk, named);
    let _ = std::fs::remove_dir_all(&dir);
}
