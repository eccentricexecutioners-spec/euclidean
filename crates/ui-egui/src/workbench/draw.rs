//! Drawing the Fusion workbench: the tab row and tool-group row on top, the left column of panels,
//! the tool panel on the right and the navigation bar at the bottom. Layout data and saved state
//! live in the parent module; the layout file is described in `docs/layout.md`.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, vec2};

use super::{Item, ResolvedLayout, ResolvedTab};
use crate::theme::Tokens;
use crate::{VectorcraftApp, chrome, dock, icons, panels, sketch, toolbar};

const TAB_ROW_HEIGHT: f32 = 34.0;
const TAB_PILL_HEIGHT: f32 = 24.0;
const GROUP_ROW_HEIGHT: f32 = 78.0;
const GROUP_CELL_HEIGHT: f32 = 66.0;
/// A group-row button: icon over a short label.
const ITEM_HEIGHT: f32 = 46.0;
const ITEM_MIN_WIDTH: f32 = 44.0;
const ITEM_MAX_WIDTH: f32 = 84.0;
const ITEM_ICON: f32 = 22.0;
const ITEM_FONT: f32 = 9.5;
/// The gap (with a divider) between two groups.
const GROUP_GAP: f32 = 11.0;
const NAV_HEIGHT: f32 = 28.0;
const BIG_BUTTON: f32 = 28.0;
/// The Fusion sketch row draws its tool buttons and icons at twice the Classic size.
const WORKBENCH_ICON_SCALE: f32 = 2.0;
const CARD_RADIUS: u8 = 14;
const HEADER_HEIGHT: f32 = 22.0;
const COLLAPSED_STRIP: f32 = 22.0;
const RAIL_WIDTH: f32 = 44.0;

/// Draw the whole Fusion chrome (everything but the canvas), in egui's panel order. The left
/// column is declared right after the app bar and the status and hint bars, so it spans the full
/// height between them (a side panel declared before later top and bottom panels takes the whole
/// remaining height; those then only span the remaining width). Then come the tab row and the
/// group row on top, the navigation bar and timeline strip at the bottom, and the tool panel on
/// the right.
///
/// Tab (the UI command `window.dock`) turns the dock and the tool column off together; with
/// either off (`chrome_on` false) the left column, tab row, group row, navigation bar, timeline
/// strip and tool panel are not drawn. The app bar, status and hint bars, step form and (while
/// sketching) the sketch toolbar stay. No collapse state is touched, so Tab again restores it.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let orig = Tokens::get(ui.ctx());
    app.ui.workbench.hits.clear();
    app.ui.workbench.tab_fills.clear();
    let lay = app.ui.workbench.layout().clone();
    // An unknown id in the layout file is skipped with one warning on the status line.
    if let Some(w) = app.ui.workbench.unshown_warning.take() {
        app.status(w);
    }
    follow_sketch_mode(app);
    let tab = active_tab(&lay, &app.ui.workbench.tab).cloned();
    // The selected-row wash is the active tab's colour, for the chrome only: the original tokens go
    // back at the end, so the canvas and Classic see them unchanged.
    let mut t = orig;
    if let Some(tab) = &tab {
        t.row_selected = orig.wash(orig.tab_color(tab.color.as_deref()), if orig.dark { 0.22 } else { 0.14 });
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::NULL, t));
    }
    let mode = app.ui.screen_mode;
    let chrome_on = app.ui.dock && app.ui.toolbar;
    if mode < 2 {
        chrome::app_bar(app, ui);
    }
    if app.ui.status_bar && mode < 3 {
        chrome::status_bar(app, ui);
        chrome::hint_bar(app, ui);
    }
    if mode < 3 && chrome_on {
        left_column(app, ui, &t, &lay, tab.as_ref());
    }
    if mode < 2 {
        if chrome_on {
            tab_row(app, ui, &t, &lay, tab.as_ref());
        }
        if sketch::active(app) {
            // Sketch mode: the sketch toolbar takes the group row's place.
            sketch::toolbar::show_scaled(app, ui, WORKBENCH_ICON_SCALE);
        } else if chrome_on {
            group_row(app, ui, &t, tab.as_ref());
        }
    }
    if mode < 3 && chrome_on {
        // The first bottom panel declared is the lowest: the navigation bar sits above the status
        // and hint bars, and the timeline strip, declared after it, above it.
        nav_bar(app, ui, &t, &lay);
        if app.session.doc().is_ok() {
            super::timeline_strip::show(app, ui, &t);
        }
        tool_panel(app, ui, &t, &lay, tab.as_ref());
    }
    super::step_form::show(app, ui.ctx());
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::NULL, orig));
}

/// Round the canvas's four corners: a mask in the frame colour over each corner of `rect` (the
/// central panel), painted after the panel on the same layer, so it lies over the canvas and under
/// every window, popup and tooltip (those are on higher layers).
pub fn round_canvas(ui: &Ui, rect: Rect) {
    const R: f32 = 12.0;
    const STEPS: usize = 12;
    let t = Tokens::get(ui.ctx());
    if !rect.is_finite() || rect.width() < 2.0 * R || rect.height() < 2.0 * R {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for (corner, sx, sy) in
        [(rect.left_top(), 1.0, 1.0), (rect.right_top(), -1.0, 1.0), (rect.left_bottom(), 1.0, -1.0), (rect.right_bottom(), -1.0, -1.0)]
    {
        let centre = corner + vec2(sx * R, sy * R);
        let first = mesh.vertices.len() as u32;
        mesh.colored_vertex(corner, t.frame);
        for i in 0..=STEPS {
            let phi = std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
            mesh.colored_vertex(centre + vec2(-sx * R * phi.sin(), -sy * R * phi.cos()), t.frame);
        }
        for i in 0..STEPS as u32 {
            mesh.add_triangle(first, first + 1 + i, first + 2 + i);
        }
    }
    ui.painter().with_clip_rect(rect).add(egui::Shape::mesh(mesh));
}

/// A rounded card around one left-column entry or the tool panel's content.
fn card(t: &Tokens, ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE.fill(t.card).corner_radius(CARD_RADIUS).inner_margin(6).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

/// Entering sketch mode (from any tab, by any route) switches to the SKETCH tab, remembering the
/// tab it came from in `prev_tab`; leaving it (Finish, Escape, undo) goes back.
fn follow_sketch_mode(app: &mut VectorcraftApp) {
    let now = sketch::active(app);
    let wb = &mut app.ui.workbench;
    if now == wb.in_sketch {
        return;
    }
    wb.in_sketch = now;
    if now {
        if wb.tab != "sketch" {
            wb.prev_tab = Some(std::mem::replace(&mut wb.tab, "sketch".into()));
        }
    } else if wb.tab == "sketch"
        && let Some(prev) = wb.prev_tab.take()
        && prev != "sketch"
    {
        wb.tab = prev;
    }
}

/// The active tab: the one named by `id`, else the first.
pub(super) fn active_tab<'a>(lay: &'a ResolvedLayout, id: &str) -> Option<&'a ResolvedTab> {
    lay.tabs.iter().find(|t| t.id == id).or_else(|| lay.tabs.first())
}

fn panel_label(id: &str) -> &'static str {
    crate::state::all_panels().find(|p| p.0 == id).map_or("", |p| p.1)
}

fn tr(s: &str) -> &str {
    crate::i18n::t(s)
}

// ---------------------------------------------------------------- using items

/// What choosing an item does: select a tool, run a command (through the menu path, so one with a
/// dialog opens it), or open a panel.
pub(crate) fn use_item(app: &mut VectorcraftApp, item: &Item) {
    match item {
        Item::Tool(t) => app.select_tool(t.id),
        Item::Cmd { id, .. } => crate::menus::invoke(app, id, serde_json::json!({})),
        Item::Panel { id, .. } => open_panel(app, id),
        Item::Sep => {}
    }
}

/// The collapsed-set keys that fold the left-column entry showing panel `id`, each with whether it
/// starts folded: a section or panel is `"<id>"`, a panel inside a section `"<section>/<panel>"`.
fn left_keys(lay: &ResolvedLayout, tab: Option<&ResolvedTab>, id: &str) -> Vec<(String, bool)> {
    let entries = lay.pinned_left.iter().chain(tab.iter().flat_map(|t| &t.left));
    for e in entries {
        if e == id {
            return vec![(e.clone(), false)];
        }
        if let Some(s) = lay.sections.get(e)
            && s.panels.iter().any(|p| p == id)
        {
            return vec![(e.clone(), false), (format!("{e}/{id}"), s.collapsed.iter().any(|c| c == id))];
        }
    }
    vec![]
}

/// Folded state: the key is in the saved set XOR the entry starts folded (so a panel a section
/// folds by default is stored as "unfolded by the user" when its key is present).
fn is_folded(collapsed: &std::collections::BTreeSet<String>, key: &str, default_folded: bool) -> bool {
    collapsed.contains(key) != default_folded
}

fn set_folded(collapsed: &mut std::collections::BTreeSet<String>, key: &str, default_folded: bool, folded: bool) {
    if folded != default_folded {
        collapsed.insert(key.to_string());
    } else {
        collapsed.remove(key);
    }
}

/// Open panel `id`: unfold it when this tab's left column or pinned right panels show it, else show
/// it in the tool panel (once; a panel already there stays).
pub(crate) fn open_panel(app: &mut VectorcraftApp, id: &str) {
    let lay = app.ui.workbench.layout().clone();
    let tab = active_tab(&lay, &app.ui.workbench.tab);
    let keys = left_keys(&lay, tab, id);
    if !keys.is_empty() {
        for (key, default) in keys {
            set_folded(&mut app.ui.workbench.collapsed, &key, default, false);
            // A panel inside a section: show it (its tab, or the section's "More" row).
            if let Some((sec, _)) = key.split_once('/') {
                if default {
                    set_folded(&mut app.ui.workbench.collapsed, &format!("{sec}/more"), true, false);
                } else {
                    app.ui.workbench.section_tab.insert(sec.to_string(), id.to_string());
                }
            }
        }
    } else if tab.is_some_and(|t| t.right.iter().any(|p| p == id)) {
        if crate::floating::group_of(&app.ui, id).is_none() {
            set_folded(&mut app.ui.workbench.collapsed, &format!("right-pinned/{id}"), false, false);
            app.ui.workbench.right_collapsed = false;
        }
    } else if crate::floating::group_of(&app.ui, id).is_none() && !app.ui.workbench.right_panels.iter().any(|p| p == id) {
        app.ui.workbench.right_panels.push(id.to_string());
        app.ui.workbench.right_collapsed = false;
    }
}

// ---------------------------------------------------------------- item buttons

/// The icon of a command or panel item: the panel's own (as the dock's icon column shows it), the
/// workbench's for a few commands; `None` for the rest.
fn item_icon(item: &Item) -> Option<&'static str> {
    let name = match item {
        Item::Panel { id, .. } => {
            crate::state::ICON_PANELS.iter().find(|p| p.0 == id).map(|p| p.2).or_else(|| crate::state::DockTab::from_id(id).map(|d| d.info().2))?
        }
        Item::Cmd { id, .. } => match id.as_str() {
            "file.place.queue" => "wb-place",
            "view.fitArtboard" | "view.fitAll" => "wb-fit",
            "sketch.edit.new" | "sketch.edit.open" => "wb-sketch",
            "window.changeParameters" => "settings",
            "parameters.looks.save" => "star",
            "object.group" => "group",
            "object.ungroup" => "ungroup",
            "type.createOutlines" | "text.fitHeadline" | "type.changeCase" => "wb-type-ops",
            "file.print" => "printer",
            "file.export.pdf" | "file.export.svg" | "file.export.png" | "file.exportAs" | "file.exportSelection" => "share-2",
            _ => "wb-command",
        },
        Item::Tool(_) | Item::Sep => return None,
    };
    icons::exists(name).then_some(name)
}

/// An item's icon in `rect`: a tool's own icon, a panel's or command's icon ([`item_icon`]); else
/// its first letter in a small frame.
fn paint_item(ui: &Ui, t: &Tokens, item: &Item, rect: Rect, tint: Color32) {
    if let Some(name) = item_icon(item) {
        icons::paint(ui, name, rect, tint);
        return;
    }
    match item {
        Item::Tool(info) => icons::paint(ui, icons::tool_icon(info.icon), rect, tint),
        Item::Cmd { label, .. } | Item::Panel { label, .. } => {
            let c = rect.shrink(rect.width() * 0.12);
            ui.painter().rect_stroke(c, 3.0, Stroke::new(1.0, t.text_dim), egui::StrokeKind::Inside);
            let letter: String = tr(label).chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            ui.painter().text(c.center(), Align2::CENTER_CENTER, letter, FontId::proportional(rect.width() * 0.5), tint);
        }
        Item::Sep => {}
    }
}

fn item_label(item: &Item) -> &str {
    match item {
        Item::Tool(t) => tr(t.label),
        Item::Cmd { label, .. } | Item::Panel { label, .. } => tr(label),
        Item::Sep => "",
    }
}

fn item_active(app: &VectorcraftApp, item: &Item) -> bool {
    matches!(item, Item::Tool(t) if app.session.tool_id() == t.id)
}

/// A square icon button for `item`; true when clicked.
fn item_button(app: &VectorcraftApp, ui: &mut Ui, t: &Tokens, item: &Item, size: f32) -> (bool, Rect) {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    paint_button(app, ui, t, item, rect, &resp);
    (resp.clicked(), rect)
}

fn paint_button(app: &VectorcraftApp, ui: &Ui, t: &Tokens, item: &Item, rect: Rect, resp: &egui::Response) {
    let active = item_active(app, item);
    if active {
        ui.painter().rect_filled(rect, 3.0, t.tool_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, t.hover);
    }
    paint_item(ui, t, item, rect.shrink(rect.width() * 0.15), if active { t.text_strong } else { t.icon });
}

// ---------------------------------------------------------------- top rows

fn tab_row(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout, active: Option<&ResolvedTab>) {
    let panel = egui::Panel::top("wb_tabs")
        .exact_size(TAB_ROW_HEIGHT)
        .frame(egui::Frame::NONE.fill(t.frame).inner_margin(egui::Margin::symmetric(8, 5)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for tab in &lay.tabs {
                    let selected = active.is_some_and(|a| a.id == tab.id);
                    let hue = t.tab_color(tab.color.as_deref());
                    let galley = ui.painter().layout_no_wrap(tab.label.to_uppercase(), crate::theme::semibold(12.0), Color32::PLACEHOLDER);
                    let w = galley.size().x + 9.0 + 8.0 + 12.0 + 14.0;
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, TAB_PILL_HEIGHT), Sense::click());
                    let fill = if selected {
                        hue
                    } else if resp.hovered() {
                        t.hover
                    } else {
                        t.surface
                    };
                    let radius = (TAB_PILL_HEIGHT / 2.0) as u8;
                    let p = ui.painter();
                    p.rect_filled(rect, radius, fill);
                    if !selected {
                        p.rect_stroke(rect, radius, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                    }
                    let (dot, text) = if selected { (t.on_accent, t.on_accent) } else { (hue, t.text_dim) };
                    p.circle_filled(egui::pos2(rect.left() + 12.0 + 4.5, rect.center().y), 4.5, dot);
                    p.galley(egui::pos2(rect.left() + 12.0 + 9.0 + 8.0, rect.center().y - galley.size().y / 2.0), galley, text);
                    app.ui.workbench.hits.push((format!("tab:{}", tab.id), resp.rect));
                    app.ui.workbench.tab_fills.push((tab.id.clone(), fill));
                    if resp.clicked() {
                        app.ui.workbench.prev_tab = Some(app.ui.workbench.tab.clone()).filter(|p| !p.is_empty());
                        app.ui.workbench.tab = tab.id.clone();
                    }
                }
            });
        });
    app.ui.workbench.hits.push(("panel:tabs".into(), panel.response.rect));
}

/// The width of a group-row button for `label` (its short label, at [`ITEM_FONT`]).
fn item_width(ui: &Ui, label: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(label.to_string(), crate::theme::semibold(ITEM_FONT), Color32::PLACEHOLDER);
    (galley.size().x + 8.0).clamp(ITEM_MIN_WIDTH, ITEM_MAX_WIDTH)
}

/// A tool's or command's label as the button under its icon shows it: short, from the
/// translated label (an English "… Tool" loses the word; a few long ones have their own).
pub(crate) fn short_label(item: &Item) -> String {
    let own = match item {
        Item::Tool(t) => match t.id {
            "directSelection" => Some("Direct"),
            "groupSelection" => Some("Group Sel."),
            "magicWand" => Some("Wand"),
            "livePaintSelection" => Some("Paint Sel."),
            "perspectiveSelection" => Some("Persp. Sel."),
            "lineSegment" => Some("Line"),
            "roundedRectangle" => Some("Rounded"),
            "rectangularGrid" => Some("Rect Grid"),
            "addAnchor" => Some("Add Point"),
            "deleteAnchor" => Some("Del Point"),
            "anchorPoint" => Some("Anchor"),
            "freeTransform" => Some("Free Tf"),
            "shapeBuilder" => Some("Builder"),
            "livePaintBucket" => Some("Bucket"),
            "perspectiveGrid" => Some("Persp. Grid"),
            "typeOnPath" => Some("On Path"),
            "verticalTypeOnPath" => Some("V. On Path"),
            "areaType" => Some("Area"),
            "verticalAreaType" => Some("V. Area"),
            "verticalType" => Some("Vertical"),
            "symbolSprayer" => Some("Sprayer"),
            "printTiling" => Some("Tiling"),
            _ => None,
        },
        Item::Cmd { id, .. } => match id.as_str() {
            "file.place.queue" => Some("Place"),
            "object.group" => Some("Group"),
            "object.ungroup" => Some("Ungroup"),
            "type.createOutlines" => Some("Outlines"),
            "text.fitHeadline" => Some("Fit"),
            "type.changeCase" => Some("Case"),
            "sketch.edit.new" => Some("New Sketch"),
            "sketch.edit.open" => Some("Edit Sketch"),
            "window.changeParameters" => Some("Change…"),
            "file.print" => Some("Print"),
            "file.export.pdf" => Some("PDF"),
            "file.export.svg" => Some("SVG"),
            "file.export.png" => Some("PNG"),
            "file.exportAs" => Some("Export…"),
            "file.exportSelection" => Some("Selection"),
            _ => None,
        },
        Item::Panel { id, .. } => match id.as_str() {
            "imageTrace" => Some("Trace"),
            "separations" => Some("Seps"),
            "flattenerPreview" => Some("Flattener"),
            "charStyles" => Some("Char Styles"),
            "paraStyles" => Some("Para Styles"),
            "openType" => Some("OpenType"),
            _ => None,
        },
        Item::Sep => None,
    };
    if let Some(own) = own
        && crate::i18n::current().code() == "en"
    {
        return own.to_string();
    }
    let label = item_label(item);
    label.strip_suffix(" Tool").unwrap_or(label).trim_end_matches('…').to_string()
}

/// What one group of the row shows: its visible buttons (folded to fit) and their widths.
struct GroupPlan {
    visible: usize,
    widths: Vec<f32>,
    caption_w: f32,
}

/// Fold the groups' visible items until the row fits `available`: the group with the most
/// buttons loses its last one, again and again; every group keeps at least one.
fn plan_groups(ui: &Ui, groups: &[super::ResolvedGroup], available: f32) -> Vec<GroupPlan> {
    let mut plans: Vec<GroupPlan> = groups
        .iter()
        .map(|g| {
            let widths: Vec<f32> = g.show.iter().map(|it| item_width(ui, &short_label(it))).collect();
            let caption = ui.painter().layout_no_wrap(format!("{} ▾", g.label), FontId::proportional(10.5), Color32::PLACEHOLDER).size().x + 6.0;
            GroupPlan { visible: widths.len(), widths, caption_w: caption }
        })
        .collect();
    let total = |plans: &[GroupPlan]| -> f32 {
        plans
            .iter()
            .enumerate()
            .map(|(i, p)| p.widths.iter().take(p.visible).sum::<f32>().max(p.caption_w) + if i > 0 { GROUP_GAP } else { 0.0 })
            .sum()
    };
    while total(&plans) > available {
        let Some((i, _)) = plans.iter().enumerate().filter(|(_, p)| p.visible > 1).max_by_key(|(_, p)| p.visible) else { break };
        if let Some(p) = plans.get_mut(i) {
            p.visible -= 1;
        }
    }
    plans
}

/// The tool-group row of the active tab, Fusion style: every group shows its items as buttons
/// (icon over a short label); the caption under them opens the drop-down with every item of the
/// group. On a narrow window the last buttons of the fullest groups fold into their drop-downs.
fn group_row(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, tab: Option<&ResolvedTab>) {
    let hue = t.tab_color(tab.and_then(|t| t.color.as_deref()));
    egui::Panel::top("wb_groups")
        .exact_size(GROUP_ROW_HEIGHT)
        .frame(egui::Frame::NONE.fill(t.frame).inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            let container = egui::Frame::NONE
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.border))
                .corner_radius(14)
                .inner_margin(egui::Margin::symmetric(10, 3))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(GROUP_CELL_HEIGHT);
                    let available = ui.available_width();
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let Some(tab) = tab else { return };
                        let plans = plan_groups(ui, &tab.groups, available);
                        for (i, (g, plan)) in tab.groups.iter().zip(&plans).enumerate() {
                            if i > 0 {
                                let (r, _) = ui.allocate_exact_size(vec2(GROUP_GAP, GROUP_CELL_HEIGHT - 12.0), Sense::hover());
                                ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, t.divider));
                            }
                            let buttons_w: f32 = plan.widths.iter().take(plan.visible).sum();
                            let w = buttons_w.max(plan.caption_w);
                            let (rect, _) = ui.allocate_exact_size(vec2(w, GROUP_CELL_HEIGHT), Sense::hover());
                            // The buttons, centred when the caption is wider.
                            let mut x = rect.left() + (w - buttons_w) / 2.0;
                            let mut chosen: Option<Item> = None;
                            for (k, item) in g.show.iter().take(plan.visible).enumerate() {
                                let bw = plan.widths.get(k).copied().unwrap_or(ITEM_MIN_WIDTH);
                                let brect = Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), vec2(bw, ITEM_HEIGHT));
                                x += bw;
                                let id = item.id().unwrap_or_default().to_string();
                                let resp = ui.interact(brect, ui.id().with(("wb-item", &g.id, &id)), Sense::click());
                                paint_row_button(app, ui, t, hue, item, brect, &resp);
                                let label = item_label(item).to_string();
                                let resp = resp.on_hover_text(match item {
                                    Item::Tool(info) => info.shortcut.map_or(label.clone(), |sc| format!("{label} ({sc})")),
                                    Item::Cmd { shortcut: Some(sc), .. } => format!("{label} ({sc})"),
                                    _ => label.clone(),
                                });
                                app.ui.workbench.hits.push((format!("item:{id}"), brect));
                                if k == 0 {
                                    app.ui.workbench.hits.push((format!("group:{}", g.id), brect));
                                }
                                if resp.clicked() {
                                    chosen = Some(item.clone());
                                }
                            }
                            // The caption under the buttons: the group's name and ▾, opening the drop-down.
                            let caption = format!("{} ▾", g.label);
                            let galley = ui.painter().layout_no_wrap(caption, FontId::proportional(10.5), Color32::PLACEHOLDER);
                            let label_rect = Rect::from_min_max(egui::pos2(rect.left(), rect.top() + 2.0 + ITEM_HEIGHT), rect.right_bottom());
                            let lresp = ui.interact(label_rect, ui.id().with(("wb-label", &g.id)), Sense::click());
                            let colour = if lresp.hovered() { t.text_strong } else { t.text_dim };
                            ui.painter().galley(label_rect.center() - galley.size() / 2.0, galley, colour);
                            app.ui.workbench.hits.push((format!("label:{}", g.id), label_rect));
                            egui::Popup::menu(&lresp).show(|ui| {
                                crate::widgets::menu_scroll(ui, |ui| {
                                    ui.set_min_width(280.0);
                                    for item in &g.items {
                                        if matches!(item, Item::Sep) {
                                            ui.separator();
                                        } else if dropdown_row(app, ui, t, item) {
                                            chosen = Some(item.clone());
                                            ui.close();
                                        }
                                    }
                                });
                            });
                            if let Some(item) = chosen {
                                use_item(app, &item);
                            }
                        }
                    });
                });
            // The 3 pt strip of the tab's colour across the container's top edge.
            let r = container.response.rect;
            let strip = Rect::from_min_size(r.min, vec2(r.width(), 3.0));
            let top = egui::CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 };
            ui.painter().with_clip_rect(r).rect_filled(strip, top, hue);
        });
}

/// One button of the group row: the icon over its short label; the active tool is filled with
/// the tab's colour, a hovered one washed with it.
fn paint_row_button(app: &VectorcraftApp, ui: &Ui, t: &Tokens, hue: Color32, item: &Item, rect: Rect, resp: &egui::Response) {
    let active = item_active(app, item);
    if active {
        ui.painter().rect_filled(rect.shrink(1.0), 7.0, hue);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect.shrink(1.0), 7.0, t.wash(hue, 0.18));
    }
    let icon = Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 4.0 + ITEM_ICON / 2.0), vec2(ITEM_ICON, ITEM_ICON));
    let (ink, text) = if active { (t.on_accent, t.on_accent) } else { (t.icon, t.text) };
    paint_item(ui, t, item, icon, ink);
    let label = short_label(item);
    let galley = ui.painter().layout_no_wrap(label, crate::theme::semibold(ITEM_FONT), text);
    let pos = egui::pos2(rect.center().x - galley.size().x.min(rect.width() - 4.0) / 2.0, rect.bottom() - 3.0 - galley.size().y);
    ui.painter().with_clip_rect(rect).galley(pos, galley, text);
}

/// One row of a group's dropdown: icon, label, shortcut at the right edge; true when clicked.
fn dropdown_row(app: &VectorcraftApp, ui: &mut Ui, t: &Tokens, item: &Item) -> bool {
    let w = ui.available_width().max(280.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, t.hover);
    }
    let active = item_active(app, item);
    let ir = Rect::from_min_size(rect.min + vec2(6.0, 3.0), vec2(18.0, 18.0));
    paint_item(ui, t, item, ir, t.icon);
    let colour = if active { t.text_strong } else { t.text };
    ui.painter().text(egui::pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, item_label(item), FontId::proportional(12.5), colour);
    let shortcut = match item {
        Item::Tool(info) => info.shortcut.map(str::to_string),
        Item::Cmd { shortcut, .. } => shortcut.clone(),
        _ => None,
    };
    if let Some(sc) = shortcut {
        ui.painter().text(egui::pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, sc, FontId::proportional(11.5), t.text_dim);
    }
    resp.clicked()
}

// ---------------------------------------------------------------- navigation bar

fn nav_bar(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout) {
    let panel = egui::Panel::bottom("wb_nav")
        .exact_size(NAV_HEIGHT)
        .frame(egui::Frame::NONE.fill(t.frame).inner_margin(egui::Margin::symmetric(8, 2)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for item in &lay.nav {
                    if matches!(item, Item::Sep) {
                        ui.add_space(6.0);
                        continue;
                    }
                    let (clicked, rect) = item_button(app, ui, t, item, 22.0);
                    app.ui.workbench.hits.push((format!("nav:{}", item_label(item)), rect));
                    ui.interact(rect, ui.id().with(("wb-nav-tip", item_label(item))), Sense::hover()).on_hover_text(item_label(item));
                    if clicked {
                        use_item(app, item);
                    }
                }
            });
        });
    app.ui.workbench.hits.push(("panel:nav".into(), panel.response.rect));
}

// ---------------------------------------------------------------- panel headers

/// What a panel header asked for.
#[derive(Default)]
struct HeaderOut {
    toggled: bool,
    closed: bool,
}

/// A section or panel header row: fold arrow and `label` (click folds when `fold` is `Some`), the
/// panel's ≡ menu when `menu_panel` is set, and a × when `closable`.
#[allow(clippy::too_many_arguments)]
fn header(
    app: &mut VectorcraftApp,
    ui: &mut Ui,
    t: &Tokens,
    key: &str,
    label: &str,
    fold: Option<bool>,
    menu_panel: Option<&str>,
    closable: bool,
    depth: f32,
) -> HeaderOut {
    let mut out = HeaderOut::default();
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), HEADER_HEIGHT));
    let mut right = rect.right() - 4.0;
    let slot = |ui: &Ui, right: &mut f32, name: &str| {
        let r = Rect::from_min_size(egui::pos2(*right - 18.0, rect.center().y - 9.0), vec2(18.0, 18.0));
        *right -= 20.0;
        (r, ui.id().with((name, key)))
    };
    if closable {
        let (r, id) = slot(ui, &mut right, "wb-close");
        let resp = ui.interact(r, id, Sense::click());
        let colour = if resp.hovered() { t.text_strong } else { t.text_dim };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "×", FontId::proportional(15.0), colour);
        let resp = resp.on_hover_text(tl!("Close Panel"));
        app.ui.workbench.hits.push((format!("close:{}", key.trim_start_matches("right/")), r));
        out.closed = resp.clicked();
    }
    if let Some(pid) = menu_panel {
        let (r, id) = slot(ui, &mut right, "wb-menu");
        let resp = ui.interact(r, id, Sense::click());
        icons::paint(ui, "menu", r.shrink(2.0), if resp.hovered() { t.text_strong } else { t.text_dim });
        let resp = resp.on_hover_text(tl!("Panel menu"));
        egui::Popup::menu(&resp).show(|ui| {
            crate::widgets::menu_scroll(ui, |ui| {
                ui.set_min_width(220.0);
                panels::panel_menu_items(app, ui, pid);
            });
        });
    }
    let main = Rect::from_min_max(rect.min, egui::pos2(right, rect.bottom()));
    let resp = ui.interact(main, ui.id().with(("wb-head", key)), Sense::click());
    if resp.hovered() && fold.is_some() {
        ui.painter().rect_filled(main, 6.0, t.hover);
    }
    let arrow = match fold {
        Some(true) => "▸ ",
        Some(false) => "▾ ",
        None => "",
    };
    ui.painter().text(
        egui::pos2(main.left() + 8.0 + depth, main.center().y),
        Align2::LEFT_CENTER,
        format!("{arrow}{label}"),
        FontId::proportional(12.0),
        t.text_strong,
    );
    app.ui.workbench.hits.push((format!("section:{key}"), main));
    out.toggled = fold.is_some() && resp.clicked();
    out
}

/// One panel's body, in a unique id scope.
fn body(app: &mut VectorcraftApp, ui: &mut Ui, host: &str, id: &str) {
    let tall = if id == "layers" { 200.0 } else { 400.0 };
    ui.push_id((host, id), |ui| dock::panel_body(app, ui, id, ui.available_width(), tall));
}

// ---------------------------------------------------------------- left column

fn left_column(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout, tab: Option<&ResolvedTab>) {
    let frame = egui::Frame::NONE.fill(t.frame).inner_margin(8);
    if app.ui.workbench.left_collapsed {
        left_rail(app, ui, t, lay, egui::Frame::NONE.fill(t.frame));
        return;
    }
    let resp = egui::Panel::left("wb_left").resizable(true).default_size(app.ui.workbench.left_width).size_range(220.0..=600.0).frame(frame).show(
        ui,
        |ui| {
            // A slim header row: the button that folds the whole column to a rail.
            ui.allocate_ui_with_layout(vec2(ui.available_width(), HEADER_HEIGHT), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                let resp = ui.button("\u{2039}").on_hover_text(tl!("Collapse panel column"));
                app.ui.workbench.hits.push(("left:collapse".into(), resp.rect));
                if resp.clicked() {
                    app.ui.workbench.left_collapsed = true;
                }
            });
            // Both ways: a panel wider than the column (Parameters' table) scrolls inside it instead
            // of widening the column (egui grows a side panel to its content).
            let width = ui.available_width();
            egui::ScrollArea::both().id_salt("wb-left-scroll").auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(width);
                let entries = lay.pinned_left.iter().chain(tab.iter().flat_map(|t| &t.left));
                for id in entries {
                    left_entry(app, ui, t, lay, id);
                }
            });
        },
    );
    let rect = resp.response.rect;
    app.ui.workbench.hits.push(("panel:left".into(), rect));
    let w = rect.width();
    if w.is_finite() && w >= 1.0 {
        app.ui.workbench.left_width = w;
    }
}

/// The collapsed left column: a narrow rail with an expand button and one icon per pinned entry.
/// (Its own panel id, so the resizable column's saved width is left alone.)
fn left_rail(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout, frame: egui::Frame) {
    let mut expand = false;
    let mut unfold: Option<String> = None;
    let resp = egui::Panel::left("wb_left_rail").resizable(false).exact_size(RAIL_WIDTH).frame(frame).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            let resp = ui.button("\u{203A}").on_hover_text(tl!("Expand panel column"));
            app.ui.workbench.hits.push(("left:expand".into(), resp.rect));
            if resp.clicked() {
                expand = true;
            }
            ui.add_space(4.0);
            for id in &lay.pinned_left {
                let (panel, label) = match lay.sections.get(id) {
                    Some(sec) => (sec.panels.first().cloned().unwrap_or_else(|| id.clone()), sec.label.as_str()),
                    None => (id.clone(), panel_label(id)),
                };
                let item = Item::Panel { id: panel, label: label.to_string() };
                let (rect, resp) = ui.allocate_exact_size(vec2(BIG_BUTTON, BIG_BUTTON), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 3.0, t.hover);
                }
                paint_item(ui, t, &item, rect.shrink(rect.width() * 0.15), t.icon);
                let resp = resp.on_hover_text(tr(label));
                app.ui.workbench.hits.push((format!("rail:{id}"), rect));
                if resp.clicked() {
                    expand = true;
                    unfold = Some(id.clone());
                }
            }
        });
    });
    app.ui.workbench.hits.push(("panel:left".into(), resp.response.rect));
    if let Some(id) = unfold {
        set_folded(&mut app.ui.workbench.collapsed, &id, false, false);
    }
    if expand {
        app.ui.workbench.left_collapsed = false;
    }
}

/// A left-column entry. Folded state is [`is_folded`] over `workbench.collapsed`: a top-level
/// entry's key is its id (present = folded); a panel inside a section is `"<section>/<panel>"`,
/// present = flipped from the section's default (so `colour/colorGuide` present = unfolded).
fn left_entry(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout, id: &str) {
    let visible = match lay.sections.get(id) {
        Some(sec) => sec.panels.iter().any(|p| crate::floating::group_of(&app.ui, p).is_none()),
        None => crate::floating::group_of(&app.ui, id).is_none(),
    };
    if !visible {
        return;
    }
    card(t, ui, |ui| left_entry_body(app, ui, t, lay, id));
    ui.add_space(8.0);
}

fn left_entry_body(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout, id: &str) {
    if let Some(sec) = lay.sections.get(id) {
        let shown: Vec<&String> = sec.panels.iter().filter(|p| crate::floating::group_of(&app.ui, p).is_none()).collect();
        if shown.is_empty() {
            return;
        }
        let folded = is_folded(&app.ui.workbench.collapsed, id, false);
        let h = header(app, ui, t, id, &sec.label, Some(folded), None, false, 0.0);
        if h.toggled {
            set_folded(&mut app.ui.workbench.collapsed, id, false, !folded);
        }
        if !folded {
            let (more, tabbed): (Vec<&String>, Vec<&String>) = shown.into_iter().partition(|p| sec.collapsed.iter().any(|c| c == *p));
            if tabbed.is_empty() {
                for p in more {
                    panel_section(app, ui, t, &format!("{id}/{p}"), p, true, 10.0, id);
                }
                return;
            }
            let active = active_section_tab(&app.ui.workbench, id, &tabbed);
            section_tab_strip(app, ui, t, id, &tabbed, &active);
            body(app, ui, id, &active);
            if !more.is_empty() {
                let key = format!("{id}/more");
                let folded = is_folded(&app.ui.workbench.collapsed, &key, true);
                if header(app, ui, t, &key, tl!("More"), Some(folded), None, false, 10.0).toggled {
                    set_folded(&mut app.ui.workbench.collapsed, &key, true, !folded);
                }
                if !folded {
                    for p in more {
                        panel_section(app, ui, t, &format!("{id}/{p}"), p, true, 20.0, id);
                    }
                }
            }
        }
    } else if crate::floating::group_of(&app.ui, id).is_none() {
        panel_section(app, ui, t, id, id, false, 0.0, "wb-left");
    }
}

/// The panel a composite section's tab strip shows: the saved choice if it is still a tab, else the first.
fn active_section_tab(wb: &super::WorkbenchState, section: &str, tabbed: &[&String]) -> String {
    let saved = wb.section_tab.get(section);
    tabbed.iter().find(|p| Some(**p) == saved).or(tabbed.first()).map(|p| (*p).clone()).unwrap_or_default()
}

/// The row of small tabs of a composite section, one per panel; a click records the choice.
fn section_tab_strip(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, section: &str, tabbed: &[&String], active: &str) {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(6.0);
        for p in tabbed {
            let resp = ui.selectable_label(p.as_str() == active, egui::RichText::new(tr(panel_label(p))).size(11.5).color(t.text_strong));
            app.ui.workbench.hits.push((format!("sectiontab:{section}/{p}"), resp.rect));
            if resp.clicked() {
                chosen = Some((*p).clone());
            }
        }
    });
    if let Some(p) = chosen {
        app.ui.workbench.section_tab.insert(section.to_string(), p);
    }
}

#[allow(clippy::too_many_arguments)]
fn panel_section(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, key: &str, id: &str, default: bool, depth: f32, host: &str) {
    let folded = is_folded(&app.ui.workbench.collapsed, key, default);
    let h = header(app, ui, t, key, tr(panel_label(id)), Some(folded), Some(id), false, depth);
    if h.toggled {
        set_folded(&mut app.ui.workbench.collapsed, key, default, !folded);
    }
    if !folded {
        body(app, ui, host, id);
    }
}

// ---------------------------------------------------------------- tool panel

fn tool_panel(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout, tab: Option<&ResolvedTab>) {
    let frame = egui::Frame::NONE.fill(t.frame).inner_margin(8);
    if app.ui.workbench.right_collapsed {
        egui::Panel::right("wb_tool_panel").resizable(false).exact_size(COLLAPSED_STRIP).frame(egui::Frame::NONE.fill(t.frame)).show(ui, |ui| {
            let resp = ui.button("«").on_hover_text(tl!("Expand tool panel"));
            app.ui.workbench.hits.push(("tool-panel:expand".into(), resp.rect));
            if resp.clicked() {
                app.ui.workbench.right_collapsed = false;
            }
        });
        return;
    }
    let resp =
        egui::Panel::right("wb_tool_panel").resizable(true).default_size(app.ui.workbench.right_width).size_range(220.0..=600.0).frame(frame).show(
            ui,
            |ui| {
                let width = ui.available_width();
                egui::ScrollArea::both().id_salt("wb-tool-scroll").auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(width);
                    // This tab's pinned right panels, above the tool's own card.
                    for id in tab.iter().flat_map(|t| &t.right) {
                        if crate::floating::group_of(&app.ui, id).is_some() {
                            continue;
                        }
                        card(t, ui, |ui| panel_section(app, ui, t, &format!("right-pinned/{id}"), id, false, 0.0, "wb-right-pinned"));
                        ui.add_space(8.0);
                    }
                    card(t, ui, |ui| {
                        let tool = app.session.tool_id().to_string();
                        ui.horizontal(|ui| {
                            let name = vectorcraft_tools::tool_info(&tool).map_or(tool.as_str(), |i| tr(i.label));
                            ui.label(egui::RichText::new(name).strong().size(13.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let resp = ui.button("»").on_hover_text(tl!("Collapse tool panel"));
                                app.ui.workbench.hits.push(("tool-panel:collapse".into(), resp.rect));
                                if resp.clicked() {
                                    app.ui.workbench.right_collapsed = true;
                                }
                            });
                        });
                        ui.horizontal_wrapped(|ui| toolbar::control_bar_options(app, ui));
                        if ui.button(tl!("Tool Options…")).clicked()
                            && let Err(e) = toolbar::open_options(app, &tool)
                        {
                            app.status(e);
                        }
                        ui.add_space(6.0);
                        let ids = app.ui.workbench.right_panels.clone();
                        let mut closed = None;
                        for id in &ids {
                            // A panel is drawn in one place only: not when floating, nor when the left column or the pinned right panels show it.
                            let pinned_right = tab.is_some_and(|t| t.right.contains(id));
                            if crate::floating::group_of(&app.ui, id).is_some() || !left_keys(lay, tab, id).is_empty() || pinned_right {
                                continue;
                            }
                            let key = format!("right/{id}");
                            if header(app, ui, t, &key, tr(panel_label(id)), None, Some(id), true, 0.0).closed {
                                closed = Some(id.clone());
                            }
                            body(app, ui, "wb-right", id);
                        }
                        if let Some(id) = closed {
                            app.ui.workbench.right_panels.retain(|p| *p != id);
                        }
                    });
                });
            },
        );
    let w = resp.response.rect.width();
    if w.is_finite() && w >= 1.0 {
        app.ui.workbench.right_width = w;
    }
}
