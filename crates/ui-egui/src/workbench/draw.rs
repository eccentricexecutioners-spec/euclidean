//! Drawing the Fusion workbench: the tab row and tool-group row on top, the left column of panels,
//! the tool panel on the right and the navigation bar at the bottom. Layout data and saved state
//! live in the parent module; the layout file is described in `docs/layout.md`.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, vec2};

use super::{Item, ResolvedLayout, ResolvedTab};
use crate::theme::Tokens;
use crate::{VectorcraftApp, chrome, dock, icons, panels, sketch, toolbar};

const TAB_ROW_HEIGHT: f32 = 26.0;
const GROUP_ROW_HEIGHT: f32 = 58.0;
const NAV_HEIGHT: f32 = 28.0;
const BIG_BUTTON: f32 = 28.0;
const HEADER_HEIGHT: f32 = 22.0;
const COLLAPSED_STRIP: f32 = 22.0;

/// Draw the whole Fusion chrome (everything but the canvas), in egui's panel order: top panels
/// (menu bar, tabs, groups), bottom panels (status, hint, navigation), then left and right.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    app.ui.workbench.hits.clear();
    let lay = app.ui.workbench.layout().clone();
    // An unknown id in the layout file is skipped with one warning on the status line.
    if let Some(w) = app.ui.workbench.unshown_warning.take() {
        app.status(w);
    }
    follow_sketch_mode(app);
    let tab = active_tab(&lay, &app.ui.workbench.tab).cloned();
    let mode = app.ui.screen_mode;
    if mode < 2 {
        chrome::app_bar(app, ui);
        tab_row(app, ui, &t, &lay, tab.as_ref());
        if sketch::active(app) {
            // Sketch mode: the sketch toolbar takes the group row's place.
            sketch::toolbar::show(app, ui);
        } else {
            group_row(app, ui, &t, tab.as_ref());
        }
    }
    if app.ui.status_bar && mode < 3 {
        chrome::status_bar(app, ui);
        chrome::hint_bar(app, ui);
    }
    if mode < 3 {
        // The first bottom panel declared is the lowest: the navigation bar sits above the status
        // and hint bars, and the timeline strip, declared after it, above it.
        nav_bar(app, ui, &t, &lay);
        if app.session.doc().is_ok() {
            super::timeline_strip::show(app, ui, &t);
        }
        left_column(app, ui, &t, &lay, tab.as_ref());
        tool_panel(app, ui, &t, &lay, tab.as_ref());
    }
    super::step_form::show(app, ui.ctx());
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
fn active_tab<'a>(lay: &'a ResolvedLayout, id: &str) -> Option<&'a ResolvedTab> {
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

/// Open panel `id`: unfold it when this tab's left column shows it, else show it in the tool panel
/// (once; a panel already there stays).
pub(crate) fn open_panel(app: &mut VectorcraftApp, id: &str) {
    let lay = app.ui.workbench.layout().clone();
    let tab = active_tab(&lay, &app.ui.workbench.tab);
    let keys = left_keys(&lay, tab, id);
    if !keys.is_empty() {
        for (key, default) in keys {
            set_folded(&mut app.ui.workbench.collapsed, &key, default, false);
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
            "sketch.edit.new" => "wb-sketch",
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
    egui::Panel::top("wb_tabs")
        .exact_size(TAB_ROW_HEIGHT)
        .frame(egui::Frame::NONE.fill(t.panel_darker).inner_margin(egui::Margin::symmetric(8, 2)).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for tab in &lay.tabs {
                    let selected = active.is_some_and(|a| a.id == tab.id);
                    let resp = ui.selectable_label(selected, egui::RichText::new(&tab.label).strong().size(12.5));
                    app.ui.workbench.hits.push((format!("tab:{}", tab.id), resp.rect));
                    if resp.clicked() {
                        app.ui.workbench.prev_tab = Some(app.ui.workbench.tab.clone()).filter(|p| !p.is_empty());
                        app.ui.workbench.tab = tab.id.clone();
                    }
                }
            });
        });
}

fn group_row(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, tab: Option<&ResolvedTab>) {
    egui::Panel::top("wb_groups")
        .exact_size(GROUP_ROW_HEIGHT)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(8, 2)).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let Some(tab) = tab else { return };
                for (i, g) in tab.groups.iter().enumerate() {
                    if i > 0 {
                        let (r, _) = ui.allocate_exact_size(vec2(9.0, GROUP_ROW_HEIGHT - 12.0), Sense::hover());
                        ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, t.divider));
                    }
                    let first = g.items.iter().find(|i| !matches!(i, Item::Sep));
                    let caption = format!("{} ▾", g.label);
                    let galley = ui.painter().layout_no_wrap(caption, FontId::proportional(10.5), t.text_dim);
                    let w = galley.size().x.max(BIG_BUTTON) + 14.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(w, GROUP_ROW_HEIGHT - 6.0), Sense::hover());
                    let button =
                        Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + BIG_BUTTON / 2.0 + 2.0), vec2(BIG_BUTTON, BIG_BUTTON));
                    let label_rect = Rect::from_min_max(egui::pos2(rect.left(), button.bottom() + 1.0), rect.right_bottom());
                    if let Some(item) = first {
                        let resp = ui.interact(button, ui.id().with(("wb-big", &g.id)), Sense::click());
                        paint_button(app, ui, t, item, button, &resp);
                        let resp = resp.on_hover_text(item_label(item));
                        app.ui.workbench.hits.push((format!("group:{}", g.id), button));
                        if resp.clicked() {
                            use_item(app, item);
                        }
                    }
                    let lresp = ui.interact(label_rect, ui.id().with(("wb-label", &g.id)), Sense::click());
                    let colour = if lresp.hovered() { t.text_strong } else { t.text_dim };
                    ui.painter().galley(label_rect.center() - galley.size() / 2.0, galley, colour);
                    app.ui.workbench.hits.push((format!("label:{}", g.id), label_rect));
                    let mut chosen: Option<Item> = None;
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
    egui::Panel::bottom("wb_nav")
        .exact_size(NAV_HEIGHT)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(8, 2)).stroke(Stroke::new(1.0, t.border)))
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
    ui.painter().rect_filled(rect, 0.0, t.panel_darker);
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
        ui.painter().rect_filled(main, 0.0, t.hover);
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
    let tall = if id == "layers" { 320.0 } else { 400.0 };
    ui.push_id((host, id), |ui| dock::panel_body(app, ui, id, ui.available_width(), tall));
}

// ---------------------------------------------------------------- left column

fn left_column(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout, tab: Option<&ResolvedTab>) {
    let resp = egui::Panel::left("wb_left")
        .resizable(true)
        .default_size(app.ui.workbench.left_width)
        .size_range(220.0..=600.0)
        .frame(egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.border)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("wb-left-scroll").auto_shrink([false, false]).show(ui, |ui| {
                let entries = lay.pinned_left.iter().chain(tab.iter().flat_map(|t| &t.left));
                for id in entries {
                    left_entry(app, ui, t, lay, id);
                }
            });
        });
    let w = resp.response.rect.width();
    if w.is_finite() && w >= 1.0 {
        app.ui.workbench.left_width = w;
    }
}

/// A left-column entry. Folded state is [`is_folded`] over `workbench.collapsed`: a top-level
/// entry's key is its id (present = folded); a panel inside a section is `"<section>/<panel>"`,
/// present = flipped from the section's default (so `colour/colorGuide` present = unfolded).
fn left_entry(app: &mut VectorcraftApp, ui: &mut Ui, t: &Tokens, lay: &ResolvedLayout, id: &str) {
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
            for p in shown {
                let key = format!("{id}/{p}");
                let default = sec.collapsed.iter().any(|c| c == p);
                panel_section(app, ui, t, &key, p, default, 10.0, id);
            }
        }
    } else if crate::floating::group_of(&app.ui, id).is_none() {
        panel_section(app, ui, t, id, id, false, 0.0, "wb-left");
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
    let frame = egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.border));
    if app.ui.workbench.right_collapsed {
        egui::Panel::right("wb_tool_panel").resizable(false).exact_size(COLLAPSED_STRIP).frame(frame).show(ui, |ui| {
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
                egui::ScrollArea::vertical().id_salt("wb-tool-scroll").auto_shrink([false, false]).show(ui, |ui| {
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
                        // A panel is drawn in one place only: not when floating, nor when the left column shows it.
                        if crate::floating::group_of(&app.ui, id).is_some() || !left_keys(lay, tab, id).is_empty() {
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
            },
        );
    let w = resp.response.rect.width();
    if w.is_finite() && w >= 1.0 {
        app.ui.workbench.right_width = w;
    }
}
