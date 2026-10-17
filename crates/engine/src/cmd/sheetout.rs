//! What leaves Euclidean for the laser (E4 Parts C and D): a sheet as a LightBurn project
//! (`nest.exportLightBurn`, [`super::fileio::lbrn`]) and a job package (`nest.exportJob`): one
//! folder with every sheet as SVG (mm), DXF and `.lbrn2`, a thumbnail PNG of each, and `job.json`
//! (schema `euclidean-job/1`, `docs/job-package.md`) whose empty `order` block the Mothership fills
//! when ordering is built. Nothing here talks to a network.
use super::*;
use serde_json::{Value, json};
use vectorcraft_doc::{Document, Unit};

use super::fileio::lbrn;
use super::nest::sheet_of;

const C_LB: &str = "nest.exportLightBurn";
const C_JOB: &str = "nest.exportJob";
/// The job file's schema id.
pub const JOB_SCHEMA: &str = "euclidean-job/1";
/// The longest side of a sheet's thumbnail, in pixels.
const THUMB_PX: f64 = 512.0;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "nest.exportLightBurn",
            "Export for LightBurn",
            [],
            None,
            "{artboard?: index | artboards?: [index…] (default: every sheet, else every artboard), path?} each sheet as a LightBurn project (.lbrn2): millimetres, one cut layer per colour (the nearest free LightBurn layer: C00 black, C01 blue, C02 red…), Line mode, LightBurn's default power and speed. With `path`: one sheet is written there, several as <stem>-<sheet name>.lbrn2 beside it → {files: [{artboard, name, path | text}], layers: [{artboard, colour, index, kind: cut|engrave, shapes}], warnings}",
            has_doc,
            export_lightburn
        ),
        cmd!(
            query "nest.exportJob",
            "Export Job",
            [],
            None,
            "{path: a folder (made if missing), artboards?: [index…] (default: every sheet), id?, created? (for repeatable files)} writes each sheet as <name>.svg (mm), <name>.dxf, <name>.lbrn2 and <name>.png (thumbnail), and job.json (schema euclidean-job/1: id, created, app, sheets [{name, artboard, material, width_mm, height_mm, thickness_mm, part_count, files, thumbnail}], operations [{colour, kind, lightburn_layer}], order {account, link_code, shop}: null until ordering fills them) → {path, job, files}",
            has_doc,
            export_job
        ),
    ]
}

/// The sheets to write: the given artboards, else every sheet, else every artboard.
fn boards(doc: &Document, p: &Value, cmd: &str) -> Result<Vec<usize>> {
    let n = doc.artboards.len();
    let check = |v: &Value| v.as_u64().and_then(|i| usize::try_from(i).ok()).filter(|i| *i < n).ok_or_else(|| bad(cmd, format!("no artboard {v}")));
    if let Some(v) = p.get("artboard").filter(|v| !v.is_null()) {
        return Ok(vec![check(v)?]);
    }
    if let Some(a) = p.get("artboards").and_then(Value::as_array) {
        let mut out = a.iter().map(check).collect::<Result<Vec<_>>>()?;
        out.dedup();
        if out.is_empty() {
            return Err(bad(cmd, "`artboards` is empty"));
        }
        return Ok(out);
    }
    let sheets: Vec<usize> = (0..n).filter(|i| sheet_of(doc, *i).is_some()).collect();
    if sheets.is_empty() { Ok((0..n).collect()) } else { Ok(sheets) }
}

/// A sheet's name made safe as a file name (no separators or reserved characters).
fn file_stem(name: &str, index: usize) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect();
    let s = s.trim().trim_matches('.').chars().take(80).collect::<String>();
    if s.is_empty() { format!("Sheet {}", index + 1) } else { s }
}

/// Unique file stems for `boards`, in order (a repeated sheet name gets " (2)").
fn stems(doc: &Document, boards: &[usize]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for b in boards {
        let base = file_stem(doc.artboards.get(*b).map_or("", |a| a.name.as_str()), *b);
        let mut stem = base.clone();
        let mut k = 2;
        while out.iter().any(|s| s.eq_ignore_ascii_case(&stem)) {
            stem = format!("{base} ({k})");
            k += 1;
        }
        out.push(stem);
    }
    out
}

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn layer_json(board: usize, l: &lbrn::Layer) -> Value {
    json!({"artboard": board, "colour": l.hex(), "index": l.index, "kind": l.kind.id(), "shapes": l.shapes})
}

fn export_lightburn(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").map(str::to_string);
    let doc = s.doc()?.doc.clone();
    let boards = boards(&doc, p, C_LB)?;
    let names = stems(&doc, &boards);
    let mut files = vec![];
    let mut layers = vec![];
    let mut warnings: Vec<String> = vec![];
    for (b, stem) in boards.iter().zip(&names) {
        let w = lbrn::write_sheet(&doc, *b, &format!("Euclidean {}", version())).map_err(|e| bad(C_LB, e))?;
        layers.extend(w.layers.iter().map(|l| layer_json(*b, l)));
        for x in w.warnings {
            if !warnings.contains(&x) {
                warnings.push(x);
            }
        }
        let name = format!("{stem}.lbrn2");
        match &path {
            Some(path) => {
                let target = if boards.len() == 1 { path.clone() } else { beside(path, &format!("-{stem}.lbrn2")) };
                super::fileio::write_file(&target, w.xml.as_bytes())?;
                files.push(json!({"artboard": b, "name": name, "path": target}));
            }
            None => files.push(json!({"artboard": b, "name": name, "text": w.xml})),
        }
    }
    Ok(json!({"files": files, "layers": layers, "warnings": warnings}))
}

/// `path` without its extension, then `suffix` (`a/job.lbrn2` + `-Sheet 1.lbrn2`).
fn beside(path: &str, suffix: &str) -> String {
    let p = std::path::Path::new(path);
    let stem = p.with_extension("");
    format!("{}{suffix}", stem.to_string_lossy())
}

/// A random-looking version-4 UUID from the clock, the process and the document (no random source
/// is linked in; it only needs to tell jobs apart).
fn job_id(doc: &Document, revision: u64) -> String {
    use std::hash::{Hash, Hasher};
    let mut a = std::collections::hash_map::DefaultHasher::new();
    vectorcraft_doc::metadata::now_unix().hash(&mut a);
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::process::id().hash(&mut a);
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0).hash(&mut a);
    }
    revision.hash(&mut a);
    doc.artboards.len().hash(&mut a);
    doc.artboards.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().hash(&mut a);
    let hi = a.finish();
    hi.rotate_left(17).hash(&mut a);
    let lo = a.finish();
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&hi.to_be_bytes());
    b[8..].copy_from_slice(&lo.to_be_bytes());
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Now as `2026-10-17T18:30:00Z`, or None without a clock (the web).
fn now_iso() -> Option<String> {
    let [y, mo, d, h, mi, s] = vectorcraft_doc::metadata::civil(vectorcraft_doc::metadata::now_unix()?);
    Some(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"))
}

/// The job's `job.json`: what [`export_job`] writes, from what it wrote.
pub fn job_json(id: &str, created: &str, doc_name: &str, sheets: Vec<Value>, operations: Vec<Value>) -> Value {
    json!({
        "schema": JOB_SCHEMA,
        "id": id,
        "created": created,
        "app": {"name": "Euclidean", "version": version()},
        "document": doc_name,
        "units": "mm",
        "sheets": sheets,
        "operations": operations,
        "order": {"account": null, "link_code": null, "shop": null},
    })
}

fn export_job(s: &mut Session, p: &Value) -> Result<Value> {
    let folder = str_param(p, "path").ok_or_else(|| bad(C_JOB, "`path` must be the job's folder"))?.trim_end_matches(['/', '\\']).to_string();
    if folder.is_empty() {
        return Err(bad(C_JOB, "`path` must be the job's folder"));
    }
    let (doc, revision, title) = {
        let st = s.doc()?;
        let name = st.path.as_deref().map_or_else(|| "Untitled".to_string(), super::fileio::file_name);
        (st.doc.clone(), st.revision, name)
    };
    let boards = boards(&doc, p, C_JOB)?;
    let names = stems(&doc, &boards);
    let id = str_param(p, "id").map_or_else(|| job_id(&doc, revision), str::to_string);
    let created = str_param(p, "created").map(str::to_string).or_else(now_iso).unwrap_or_default();
    super::fileio::create_dir(&folder)?;
    let join = |name: &str| format!("{folder}/{name}");
    let pt = Unit::Millimeters.to_pt(1.0);
    let mut sheets = vec![];
    let mut operations: Vec<Value> = vec![];
    let mut written = vec![];
    for (b, stem) in boards.iter().zip(&names) {
        let range = (b + 1).to_string();
        // SVG at true size (mm), DXF (mm), and the thumbnail, through the exporters.
        let mut files = vec![];
        for (format, ext, extra) in
            [("svg", "svg", json!({"sizeUnit": "mm", "responsive": false})), ("dxf", "dxf", json!({})), ("png", "png", json!({"ppi": 0}))]
        {
            let mut q = json!({"format": format, "useArtboards": true, "range": range});
            if let (Some(o), Some(e)) = (q.as_object_mut(), extra.as_object()) {
                for (k, v) in e {
                    o.insert(k.clone(), v.clone());
                }
            }
            if format == "png" {
                // At most THUMB_PX on the longer side.
                let r = doc.artboards.get(*b).map(|a| a.rect).unwrap_or_default();
                let longest = r.width().max(r.height()).max(1.0);
                q["ppi"] = json!((THUMB_PX / longest * 72.0).clamp(1.0, 300.0));
            }
            let name = format!("{stem}.{ext}");
            // The bytes back, written under the job's own name (an export to a path would add the
            // artboard's number).
            let r = s.execute("document.export", &q)?;
            let bytes = r["dataBase64"]
                .as_str()
                .and_then(vectorcraft_format::base64_decode)
                .or_else(|| r["text"].as_str().map(|t| t.as_bytes().to_vec()))
                .ok_or_else(|| bad(C_JOB, format!("the {format} export gave nothing")))?;
            super::fileio::write_file(&join(&name), &bytes)?;
            written.push(name.clone());
            files.push(name);
        }
        let w = lbrn::write_sheet(&doc, *b, &format!("Euclidean {}", version())).map_err(|e| bad(C_JOB, e))?;
        let name = format!("{stem}.lbrn2");
        super::fileio::write_file(&join(&name), w.xml.as_bytes())?;
        written.push(name.clone());
        let thumbnail = files.pop().unwrap_or_default();
        files.push(name);
        for l in &w.layers {
            if !operations.iter().any(|o| o["colour"] == json!(l.hex())) {
                operations.push(json!({"colour": l.hex(), "kind": l.kind.id(), "lightburn_layer": l.index}));
            }
        }
        let a = doc.artboards.get(*b);
        let rect = a.map(|a| a.rect).unwrap_or_default();
        let spec = sheet_of(&doc, *b).map(|x| x.0);
        let mm = |v: f64| (v / pt * 1e3).round() / 1e3;
        sheets.push(json!({
            "name": a.map_or(String::new(), |a| a.name.clone()),
            "artboard": b,
            "material": spec.as_ref().map(|s| s.material.clone()).filter(|m| !m.is_empty()),
            "width_mm": mm(rect.width()),
            "height_mm": mm(rect.height()),
            "thickness_mm": spec.as_ref().map(|s| s.thickness).filter(|t| *t > 0.0),
            "part_count": doc.art_on_artboard(rect, false).len(),
            "files": files,
            "thumbnail": thumbnail,
        }));
    }
    let job = job_json(&id, &created, &title, sheets, operations);
    let text = serde_json::to_string_pretty(&job).map_err(|e| bad(C_JOB, e.to_string()))?;
    super::fileio::write_file(&join("job.json"), text.as_bytes())?;
    written.push("job.json".into());
    Ok(json!({"path": folder, "job": job, "files": written}))
}
