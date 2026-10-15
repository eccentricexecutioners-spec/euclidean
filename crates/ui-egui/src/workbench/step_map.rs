//! Which tool, tab, colour and icon a timeline step belongs to, by the command it records.

use egui::Color32;

use super::layout::{Item, ResolvedLayout, ResolvedTab};
use crate::theme::Tokens;

/// Step commands whose recording tool has another id (the rest record under their own command).
const TOOL_OF_STEP: &[(&str, &str)] = &[
    ("shape.rectangle", "rectangle"),
    ("shape.roundedRectangle", "roundedRectangle"),
    ("shape.ellipse", "ellipse"),
    ("shape.polygon", "polygon"),
    ("shape.star", "star"),
    ("shape.line", "lineSegment"),
    ("shape.lineSegment", "lineSegment"),
    ("shape.arc", "arc"),
    ("shape.spiral", "spiral"),
    ("shape.rectangularGrid", "rectangularGrid"),
    ("shape.polarGrid", "polarGrid"),
    ("path.create", "pen"),
    ("path.appendAnchor", "pen"),
    ("path.close", "pen"),
    ("path.curvature", "curvature"),
    ("path.curvatureEdit", "curvature"),
    ("path.freehand", "pencil"),
    ("brush.freehand", "paintbrush"),
    ("path.blob", "blobBrush"),
    ("path.eraseRegion", "eraser"),
    ("path.eraseSegments", "pathEraser"),
    ("path.joinScrub", "join"),
    ("path.smoothRegion", "smooth"),
    ("path.knife", "knife"),
    ("path.split", "scissors"),
    ("path.insertAnchor", "addAnchor"),
    ("path.removeAnchor", "deleteAnchor"),
    ("path.convertAnchor", "anchorPoint"),
    ("path.mirrorCut", "mirrorCut"),
    ("path.lineCut", "lineCut"),
    ("path.rectCut", "rectCut"),
    ("path.reshape", "reshape"),
    ("path.moveAnchors", "directSelection"),
    ("path.setHandle", "directSelection"),
    ("path.reshapeSegment", "directSelection"),
    ("object.transform", "selection"),
    ("object.distort", "freeTransform"),
    ("object.liquify", "warp"),
    ("object.puppetWarp", "puppetWarp"),
    ("symbol.spray", "symbolSprayer"),
    ("symbol.adjust", "symbolShifter"),
    ("livePaint.fill", "livePaintBucket"),
    ("object.mesh.create", "mesh"),
    ("object.blend.make", "blend"),
    ("text.create", "type"),
    ("text.createInPath", "typeOnPath"),
    ("artboard.new", "artboard"),
    ("object.slice.create", "slice"),
    ("object.slice.move", "slice"),
    ("print.tiling.set", "printTiling"),
    ("stroke.widthPoint.set", "width"),
    ("shapeBuilder.merge", "shapeBuilder"),
    ("shaper.scribble", "shaper"),
    ("perspective.grid.set", "perspectiveGrid"),
    ("perspective.transform", "perspectiveSelection"),
    ("paint.editGradient", "gradient"),
    ("paint.setGradientGeom", "gradient"),
    ("appearance.copyFrom", "eyedropper"),
];

/// The catalogue tool whose use records step command `cmd` (none for a command only a menu,
/// panel or sketch runs).
pub fn step_tool(cmd: &str) -> Option<&'static str> {
    TOOL_OF_STEP.iter().find(|(c, _)| *c == cmd).map(|(_, tool)| *tool)
}

/// The first tab (layout order) that holds the step's tool or command.
pub fn step_tab<'a>(lay: &'a ResolvedLayout, cmd: &str) -> Option<&'a ResolvedTab> {
    let tool = step_tool(cmd);
    lay.tabs.iter().find(|tab| {
        tab.groups.iter().any(|g| {
            g.items.iter().any(|it| match it {
                Item::Tool(t) => Some(t.id) == tool,
                Item::Cmd { id, .. } => id == cmd,
                _ => false,
            })
        })
    })
}

/// The step's colour: its tab's, or the Draw colour when no tab holds it.
pub fn step_color(t: &Tokens, lay: &ResolvedLayout, cmd: &str) -> Color32 {
    match step_tab(lay, cmd) {
        Some(tab) => t.tab_color(tab.color.as_deref()),
        None => t.tab_draw,
    }
}

/// The icon a step wears, by the command it records: a tool's icon for what a tool makes, the
/// Color panel's for paint, else the generic command icon.
pub fn step_icon(cmd: &str) -> &'static str {
    let name = match cmd {
        "shape.rectangle" => crate::icons::tool_icon("tool-rect"),
        "shape.roundedRectangle" => crate::icons::tool_icon("tool-rounded-rect"),
        "shape.ellipse" => crate::icons::tool_icon("tool-ellipse"),
        "shape.polygon" => crate::icons::tool_icon("tool-polygon"),
        "shape.star" => crate::icons::tool_icon("tool-star"),
        "shape.line" | "shape.lineSegment" => crate::icons::tool_icon("tool-line"),
        "object.move" | "object.transform" => crate::icons::tool_icon("tool-selection"),
        "object.rotate" => crate::icons::tool_icon("tool-rotate"),
        "object.scale" => crate::icons::tool_icon("tool-scale"),
        "select.save" => "save",
        "select.editSaved" => "dc-selection",
        "file.print" => "printer",
        "path.reverse" => "dc-reverse",
        "path.removeAnchors" => "pen-tool-delete",
        "path.join" | "path.average" => "wb-path-ops",
        "object.group" => "group",
        "object.ungroup" => "ungroup",
        "object.expandShape" | "object.shape.convertToShape" => "wb-shape-convert",
        "document.rasterEffectsSettings" => "dc-fx",
        "text.areaOptions" | "type.convertToAreaType" => "dc-type-area",
        "type.pathOptions" => "dc-type-path",
        "type.orientation.vertical" => "dc-type-vertical",
        "type.orientation.horizontal" => "type",
        "text.thread.create" => "link",
        "text.thread.releaseSelection" | "text.thread.remove" => "link-2-off",
        c if (c.starts_with("text.") || c.starts_with("type.")) && step_tool(c).is_none() => "wb-type-ops",
        c if c.starts_with("imageTrace.") => "wb-image-trace",
        c if c.starts_with("livePaint.") && step_tool(c).is_none() => "dc-live-bucket",
        c if c.starts_with("object.arrange.") => "wb-arrange",
        c if c.starts_with("object.blend.") && step_tool(c).is_none() => "dc-blend",
        c if c.starts_with("object.envelope.") => "wb-envelope",
        c if c.starts_with("object.pattern.") => "wb-pattern",
        c if c.starts_with("object.repeat.") => "wb-repeat",
        c if c.starts_with("paint.") && step_tool(c).is_none() => "palette",
        c if c.starts_with("object.path.") => crate::icons::tool_icon("tool-pen"),
        c if c.starts_with("sketch.") => "wb-sketch",
        c => match step_tool(c).and_then(vectorcraft_tools::tool_info) {
            Some(info) => crate::icons::tool_icon(info.icon),
            None => "wb-command",
        },
    };
    if crate::icons::exists(name) { name } else { "wb-command" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mapped_tool_exists() {
        for (cmd, tool) in TOOL_OF_STEP {
            assert!(vectorcraft_tools::tool_info(tool).is_some(), "{cmd} maps to unknown tool {tool}");
            assert_eq!(step_tool(cmd), Some(*tool));
        }
        assert_eq!(step_tool("sketch.edit.new"), None);
    }

    #[test]
    fn mapped_steps_wear_their_tools_icon() {
        assert_eq!(step_icon("path.knife"), crate::icons::tool_icon(vectorcraft_tools::tool_info("knife").map_or("", |i| i.icon)));
        assert_eq!(step_icon("document.setup"), "wb-command");
    }

    /// Tools that record no journaled document command (selection-only and view tools).
    const NO_STEP_TOOLS: &[&str] = &["magicWand", "lasso", "livePaintSelection", "measure", "hand", "zoom", "rotateView"];

    /// Tools that record a step another tool also records, so no table row maps to them.
    const SHARED_STEP_TOOLS: &[(&str, &str)] = &[
        ("groupSelection", "object.transform"),
        ("rotate", "object.transform"),
        ("reflect", "object.transform"),
        ("scale", "object.transform"),
        ("shear", "object.transform"),
        ("twirl", "object.liquify"),
        ("pucker", "object.liquify"),
        ("bloat", "object.liquify"),
        ("scallop", "object.liquify"),
        ("crystallize", "object.liquify"),
        ("wrinkle", "object.liquify"),
        ("symbolScruncher", "symbol.adjust"),
        ("symbolSizer", "symbol.adjust"),
        ("symbolSpinner", "symbol.adjust"),
        ("symbolStainer", "symbol.adjust"),
        ("symbolScreener", "symbol.adjust"),
        ("symbolStyler", "symbol.adjust"),
        ("sliceSelection", "object.slice.move"),
        ("areaType", "text.create"),
        ("verticalType", "text.create"),
        ("verticalAreaType", "text.create"),
        ("verticalTypeOnPath", "text.createInPath"),
        ("typeOnPath", "text.createInPath"),
    ];

    fn generic(cmd: &str) -> bool {
        let i = step_icon(cmd);
        i == "wb-command" || i == "square-dashed" || !crate::icons::exists(i)
    }

    #[test]
    fn every_step_the_default_layout_can_make_has_its_own_icon() {
        let (lay, _) = super::super::resolve(&super::super::default_layout());
        // `select.*` (except the two that store a saved selection in the document) change only the
        // selection, which is no document change and so no step.
        let never = |id: &str| {
            (id.starts_with("select.") && !matches!(id, "select.save" | "select.editSaved"))
                || id.starts_with("timeline.")
                || matches!(id, "edit.undo" | "edit.redo" | "edit.undoTo" | "edit.redoTo" | "file.revert")
        };
        let mut bad: Vec<String> = vec![];
        let items = lay.tabs.iter().flat_map(|t| t.groups.iter().flat_map(|g| g.items.iter())).chain(lay.nav.iter());
        for it in items {
            match it {
                Item::Cmd { id, .. } => {
                    let Some(spec) = vectorcraft_engine::cmd::find_command(id) else { continue };
                    if spec.journal && !never(id) && generic(id) {
                        bad.push(format!("cmd {id}"));
                    }
                }
                Item::Tool(t) => {
                    if NO_STEP_TOOLS.contains(&t.id) {
                        continue;
                    }
                    let mut cmds: Vec<&str> = TOOL_OF_STEP.iter().filter(|(_, tool)| *tool == t.id).map(|(c, _)| *c).collect();
                    cmds.extend(SHARED_STEP_TOOLS.iter().filter(|(tool, _)| *tool == t.id).map(|(_, c)| *c));
                    if cmds.is_empty() {
                        bad.push(format!("tool {} has no step command", t.id));
                    }
                    for c in cmds {
                        if generic(c) {
                            bad.push(format!("tool {} step {c}", t.id));
                        }
                    }
                }
                _ => {}
            }
        }
        bad.sort();
        bad.dedup();
        assert!(bad.is_empty(), "{} generic steps:\n{}", bad.len(), bad.join("\n"));
    }
}
