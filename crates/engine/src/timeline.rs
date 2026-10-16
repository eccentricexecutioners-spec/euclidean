//! The timeline (Euclidean): the document's command steps, in order, editable. Editing a step
//! puts the document back to its base, runs the steps again with the edited one changed, and stops
//! at the first step that fails (marked red, the ones after it kept but unrun), as Fusion's
//! timeline does. The whole edit is one undo step.
//!
//! It is stored in the document, in `document.unknown["euclidean.timeline"]` (as B2 stores
//! `euclidean.params`), as plain JSON: `{version, base, steps}` (see `docs/timeline.md`). The base
//! is the document as it was before the first step (the empty document for a new one, the
//! document as opened for a file), serialized as a JSON string so a step copies it as one block of
//! bytes. Undo and redo need no code of their own: the undo history keeps whole documents, so the
//! document an undo goes back to has the steps it had then.
//!
//! A step is recorded for every journaled top-level command that changes the active document, and
//! for a committed drag ([`Session::commit_interaction`]), with the selection, current layer and
//! isolation it acted on, and the new-art template and drawing mode new art took then (Set Fill
//! with nothing selected changes the template, not the document), which a replay restores before
//! running it. Selection commands change no document and are not steps.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vectorcraft_color::Paint;
use vectorcraft_doc::{Appearance, Document, NodeId, Selection};
use vectorcraft_tools::PaintDefaults;

use crate::{DrawMode, EngineError, Result, Session};

/// Where the timeline lives in `document.unknown`.
pub const DOC_KEY: &str = "euclidean.timeline";

/// The largest base kept in the document (its JSON, in bytes). A larger document opened from a
/// file still records its steps, but they can't be edited: every step copies the base, and the
/// undo history keeps one copy per step.
pub const BASE_CAP: usize = 1 << 20;

/// Most steps kept; past it the oldest go and editing the ones left is off (their base is gone).
pub const MAX_STEPS: usize = 2000;

/// Commands that change the document without being steps of it: undo and redo move between
/// documents that already hold their steps; the timeline's own commands rewrite them.
fn not_a_step(id: &str) -> bool {
    matches!(id, "edit.undo" | "edit.redo" | "edit.undoTo" | "edit.redoTo" | "file.revert") || id.starts_with("timeline.")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub version: u32,
    /// The document before the first step, as JSON text; `None` when it was too large to keep
    /// ([`BASE_CAP`], `base_bytes` then says how large) or the oldest steps were let go.
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_bytes: Option<usize>,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// The end marker rolled back: how many steps are applied (the later ones are kept, unrun, and
    /// a new action goes in at the marker). `None`: at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<usize>,
}

impl Timeline {
    /// How many steps are applied: the marker, or all of them.
    pub fn applied(&self) -> usize {
        self.marker.map_or(self.steps.len(), |m| m.min(self.steps.len()))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Ok,
    /// Failed on the last replay: replay stopped here.
    Failed,
    /// After a failed step: kept, not run.
    Pending,
}

impl Status {
    fn is_ok(&self) -> bool {
        *self == Status::Ok
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// The command id (`object.path.offsetPath`).
    pub cmd: String,
    /// Its params as the journal records them (with the values it noted).
    pub params: Value,
    /// What the strip shows (`Offset Path 5`).
    pub label: String,
    /// The selection the step acted on.
    #[serde(default)]
    pub selection: Selection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<NodeId>,
    /// The new-art template when it wasn't the default (white fill, 1 pt black stroke).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_art: Option<NewArt>,
    #[serde(default, skip_serializing_if = "is_normal")]
    pub draw_mode: DrawMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_inside: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Status::is_ok")]
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Skipped on replay (Suppress Timeline Step); the later steps run without it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// E1: the feature this step records (`sketch`, `pen`, `appearance:2,3`…): the commands of
    /// one feature that follow each other merge into one step (Fusion records features, not
    /// clicks). `None` for a step that is one command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    /// E1: the commands merged into this step after its own, replayed after it in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Step>,
    /// E1: the name the user gave the step (Rename); shown instead of the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// E1: the objects this step (with its children) created, in id order. On replay the objects
    /// it creates again may get other ids (an earlier creation was suppressed or deleted): the
    /// later steps are pointed at the new ids, so they keep acting on the same objects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<NodeId>,
    /// E1: a step this one acted on was deleted, so its objects are gone: it fails on replay
    /// rather than acting on whatever took their ids.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub orphaned: bool,
}

/// Does `step` (or a child) act on any of `ids`?
pub fn references(step: &Step, ids: &std::collections::BTreeSet<NodeId>) -> bool {
    !ids.is_empty() && (step.selection.objects.iter().any(|id| ids.contains(id)) || names(step, ids))
}

/// Does `step` (or a child) name any of `ids` in its params, isolation or draw-inside target?
pub fn names(step: &Step, ids: &std::collections::BTreeSet<NodeId>) -> bool {
    if ids.is_empty() {
        return false;
    }
    let in_params = |p: &Value| {
        p.as_object().is_some_and(|o| {
            ["id", "ids", "sketch", "target", "into", "with"].iter().any(|k| match o.get(*k) {
                Some(Value::Number(n)) => n.as_u64().is_some_and(|i| ids.contains(&NodeId(i))),
                Some(Value::Array(a)) => a.iter().any(|v| v.as_u64().is_some_and(|i| ids.contains(&NodeId(i)))),
                _ => false,
            })
        })
    };
    step.isolation.is_some_and(|id| ids.contains(&id))
        || step.draw_inside.is_some_and(|id| ids.contains(&id))
        || in_params(&step.params)
        || step.children.iter().any(|c| names(c, ids))
}

/// Every node id of `doc`.
fn node_ids(doc: &Document) -> std::collections::BTreeSet<NodeId> {
    let mut ids = std::collections::BTreeSet::new();
    doc.walk(|n| {
        ids.insert(n.id);
    });
    ids
}

/// The ids in `after` that `before` did not have.
fn created_between(before: &Document, after: &Document) -> Vec<NodeId> {
    let was = node_ids(before);
    node_ids(after).into_iter().filter(|id| !was.contains(id)).collect()
}

/// Point `step` at the new ids of objects that were created again under other ids.
fn remap_step(step: &mut Step, map: &std::collections::BTreeMap<NodeId, NodeId>) {
    if map.is_empty() {
        return;
    }
    let m = |id: &mut NodeId| {
        if let Some(n) = map.get(id) {
            *id = *n;
        }
    };
    for id in &mut step.selection.objects {
        m(id);
    }
    for id in [&mut step.layer, &mut step.isolation, &mut step.draw_inside].into_iter().flatten() {
        m(id);
    }
    remap_params(&mut step.params, map);
    for c in &mut step.children {
        remap_step(c, map);
    }
}

/// The object ids a command's params name (`id`, `ids`, `sketch`, `target`, `into`), remapped.
fn remap_params(params: &mut Value, map: &std::collections::BTreeMap<NodeId, NodeId>) {
    let Some(obj) = params.as_object_mut() else { return };
    for key in ["id", "ids", "sketch", "target", "into", "with"] {
        match obj.get_mut(key) {
            Some(Value::Number(n)) => {
                if let Some(new) = n.as_u64().and_then(|i| map.get(&NodeId(i))) {
                    *obj.get_mut(key).unwrap_or(&mut Value::Null) = json!(new.0);
                }
            }
            Some(Value::Array(a)) => {
                for v in a.iter_mut() {
                    if let Some(new) = v.as_u64().and_then(|i| map.get(&NodeId(i))) {
                        *v = json!(new.0);
                    }
                }
            }
            _ => {}
        }
    }
}

impl Step {
    /// What the strip shows: the user's name, else the label.
    pub fn shown(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.label)
    }
}

/// The feature a command belongs to, when it is part of one: `(key, starts a new step, label)`.
/// A command that starts a feature (a new sketch, the pen's first click) always makes a step; one
/// that continues it merges into the step before it when that step is the same feature.
/// Selection-bound features (appearance, transform, type) merge only on the same objects.
pub fn feature_of(cmd: &str, selection: &Selection) -> Option<(String, bool, &'static str)> {
    let sel = || selection.objects.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(",");
    Some(match cmd {
        "sketch.new" => ("sketch".into(), true, "Sketch"),
        c if c.starts_with("sketch.") => ("sketch".into(), false, "Sketch"),
        "path.create" => ("pen".into(), true, "Pen Path"),
        "path.appendAnchor" | "path.close" => ("pen".into(), false, "Pen Path"),
        "text.create" | "text.createInPath" => (format!("type:{}", sel()), true, "Type"),
        "text.setText" | "text.insertInline" | "text.editRange" | "text.setFormat" | "text.setRangeStyle" | "text.setStyle" | "type.insert" => {
            (format!("type:{}", sel()), false, "Type")
        }
        "paint.setFill"
        | "paint.setStroke"
        | "paint.swap"
        | "paint.none"
        | "paint.default"
        | "paint.invert"
        | "paint.complement"
        | "stroke.set"
        | "stroke.setAdvanced"
        | "object.setOpacity"
        | "appearance.setItem"
        | "appearance.addFill"
        | "appearance.addStroke"
        | "appearance.addEffect" => (format!("appearance:{}", sel()), false, "Appearance"),
        "object.move" | "object.transform" | "object.transformAgain" | "object.transformEach" => (format!("transform:{}", sel()), false, "Transform"),
        c if c.starts_with("parameters.") => ("parameters".into(), false, "Parameters"),
        _ => return None,
    })
}

fn is_normal(m: &DrawMode) -> bool {
    *m == DrawMode::default()
}

/// The session's new-art template ([`PaintDefaults`]) as a step keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewArt {
    pub fill: Paint,
    pub stroke: Paint,
    pub stroke_width: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
    pub opacity: f32,
    #[serde(default)]
    pub blend: vectorcraft_color::BlendMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default)]
    pub inherited: bool,
}

impl NewArt {
    fn of(p: &PaintDefaults) -> Option<Self> {
        (*p != PaintDefaults::default()).then(|| NewArt {
            fill: p.fill.clone(),
            stroke: p.stroke.clone(),
            stroke_width: p.stroke_width,
            appearance: p.appearance.clone(),
            opacity: p.opacity,
            blend: p.blend,
            style: p.style.clone(),
            inherited: p.inherited,
        })
    }

    fn defaults(a: Option<&NewArt>) -> PaintDefaults {
        a.map_or_else(PaintDefaults::default, |a| PaintDefaults {
            fill: a.fill.clone(),
            stroke: a.stroke.clone(),
            stroke_width: a.stroke_width,
            appearance: a.appearance.clone(),
            opacity: a.opacity,
            blend: a.blend,
            style: a.style.clone(),
            inherited: a.inherited,
        })
    }
}

/// The document's timeline, if it has one.
pub fn read(doc: &Document) -> Option<Timeline> {
    serde_json::from_value(doc.unknown.get(DOC_KEY)?.clone()).ok()
}

/// Store `t` as the document's timeline.
pub fn store(doc: &mut Document, t: &Timeline) {
    if let Ok(v) = serde_json::to_value(t) {
        doc.unknown.insert(DOC_KEY.into(), v);
    }
}

/// Give `doc` the timeline `like` has (none when it has none): for comparing two documents
/// whatever steps they record.
pub fn align(doc: &mut Document, like: &Document) {
    match like.unknown.get(DOC_KEY) {
        Some(v) if doc.unknown.get(DOC_KEY) != Some(v) => {
            doc.unknown.insert(DOC_KEY.into(), v.clone());
        }
        None if doc.unknown.contains_key(DOC_KEY) => {
            doc.unknown.remove(DOC_KEY);
        }
        _ => {}
    }
}

/// `doc` without its timeline, as JSON text: a base.
fn base_of(doc: &Document) -> String {
    let mut d = doc.clone();
    if d.unknown.contains_key(DOC_KEY) {
        d.unknown.remove(DOC_KEY);
    }
    serde_json::to_string(&d).unwrap_or_default()
}

/// A step's short label: the command's label and its first plain values (`Offset Path 5`).
pub fn label_for(cmd: &str, params: &Value) -> String {
    let name = crate::cmd::find_command(cmd).map_or(cmd, |c| c.label);
    with_values(name.trim_end_matches('…').trim_end_matches("..."), params)
}

/// `name` and the first two plain values of `params` (`Rectangle 50 × 40`).
fn with_values(name: &str, params: &Value) -> String {
    let mut vals = vec![];
    // Width before height (`Rectangle 200 × 140`), then the rest in key order.
    let mut entries: Vec<(&String, &Value)> = params.as_object().into_iter().flatten().collect();
    entries.sort_by_key(|(k, _)| match k.as_str() {
        "width" => 0,
        "height" => 1,
        _ => 2,
    });
    for (k, v) in entries {
        if matches!(k.as_str(), "ids" | "id" | "created" | "x" | "y") {
            continue;
        }
        let s = match v {
            Value::Number(n) => n.as_f64().map(|f| format!("{}", (f * 100.0).round() / 100.0)),
            Value::String(s) if s.chars().count() <= 12 => Some(s.clone()),
            _ => None,
        };
        vals.extend(s);
        if vals.len() == 2 {
            break;
        }
    }
    if vals.is_empty() { name.to_string() } else { format!("{name} {}", vals.join(" × ")) }
}

/// What a step needs from before its command ran.
pub(crate) struct Before {
    uid: u64,
    doc: Arc<Document>,
    selection: Selection,
    layer: Option<NodeId>,
    isolation: Option<NodeId>,
    new_art: Option<NewArt>,
    draw_mode: DrawMode,
    draw_inside: Option<NodeId>,
    /// The newest undo step then (its document's address) and how many there were: a step is an
    /// edit that made an undo step (saving, which stamps the document, is not one).
    undo_top: Option<usize>,
    undo_len: usize,
}

fn undo_top(st: &crate::DocState) -> Option<usize> {
    st.history.undo.last().map(|e| Arc::as_ptr(&e.doc) as usize)
}

impl Before {
    pub(crate) fn uid(&self) -> u64 {
        self.uid
    }
}

impl Session {
    /// The state a step of command `id` starts from, or `None` when it would not be a step (a
    /// nested command, a replay, an interaction's preview, no document).
    pub(crate) fn timeline_before(&self, id: &str) -> Option<Before> {
        if !self.timeline_recording || self.timeline_replay || not_a_step(id) {
            return None;
        }
        let st = self.active()?;
        if st.interaction.is_some() {
            return None;
        }
        Some(Before {
            uid: st.uid,
            doc: st.doc.clone(),
            selection: st.selection.clone(),
            layer: st.active_layer,
            isolation: st.isolation,
            new_art: NewArt::of(&self.paint),
            draw_mode: self.draw_mode,
            draw_inside: self.draw_inside,
            undo_top: undo_top(st),
            undo_len: st.history.undo.len(),
        })
    }

    /// The state an interaction started from (taken before it is kept).
    pub(crate) fn timeline_before_interaction(&self, it: &crate::Interaction) -> Option<Before> {
        if !self.timeline_recording || self.timeline_replay {
            return None;
        }
        let st = self.active()?;
        Some(Before {
            uid: st.uid,
            doc: it.doc.clone(),
            selection: (*it.selection).clone(),
            layer: it.active_layer,
            isolation: it.isolation,
            new_art: NewArt::of(&self.paint),
            draw_mode: self.draw_mode,
            draw_inside: self.draw_inside,
            undo_top: undo_top(st),
            undo_len: st.history.undo.len(),
        })
    }

    /// Append a step for `cmd` (journaled with `params`) when it changed the document `before`
    /// was taken from.
    pub(crate) fn timeline_record(&mut self, before: Before, cmd: &str, params: &Value, label: Option<&str>) {
        let Some(st) = self.active_mut() else { return };
        // A new undo step: one more, or as many with a new newest one (the oldest let go). Fewer
        // is a rollback (Discard Empty Type goes back to before the text without a trace).
        let len = st.history.undo.len();
        let made_undo_step = len > before.undo_len || (len == before.undo_len && undo_top(st) != before.undo_top);
        // In an undo group (a scrubbed field) the edits after the first extend its undo step.
        if st.uid != before.uid || Arc::ptr_eq(&st.doc, &before.doc) || !(made_undo_step || st.undo_group.is_some()) {
            return;
        }
        let mut t = read(&before.doc).unwrap_or_else(|| {
            let base = base_of(&before.doc);
            let (base, base_bytes) = if base.len() <= BASE_CAP { (Some(base), None) } else { (None, Some(base.len())) };
            Timeline { version: 1, base, base_bytes, steps: vec![], marker: None }
        });
        let label = label.map_or_else(|| label_for(cmd, params), |l| with_values(l, params));
        let feature = feature_of(cmd, &before.selection);
        let created = created_between(&before.doc, &st.doc);
        let step = Step {
            cmd: cmd.to_string(),
            params: params.clone(),
            label,
            selection: before.selection,
            layer: before.layer,
            isolation: before.isolation,
            new_art: before.new_art,
            draw_mode: before.draw_mode,
            draw_inside: before.draw_inside,
            status: Status::Ok,
            error: None,
            suppressed: false,
            feature: None,
            children: vec![],
            name: None,
            created,
            orphaned: false,
        };
        // A feature's next command merges into its step (not across the marker, nor into a
        // suppressed step).
        if let Some((key, starts_new, feature_label)) = feature {
            let last_matches = t.marker.is_none() && t.steps.last().is_some_and(|l| l.feature.as_deref() == Some(&key) && !l.suppressed);
            if !starts_new && last_matches {
                if let Some(last) = t.steps.last_mut() {
                    last.created.extend(step.created.iter().copied());
                    last.children.push(step);
                    last.status = Status::Ok;
                    last.error = None;
                    // Two or more edits on a selection: the step is the feature ("Appearance"),
                    // not its first command; a sketch or pen step was named so from the start.
                    if last.name.is_none() && !matches!(key.as_str(), "sketch" | "pen") {
                        last.label = feature_label.to_string();
                    }
                }
                store(Arc::make_mut(&mut st.doc), &t);
                st.history.recost_last(&st.doc);
                return;
            }
            let mut step = step;
            // A sketch or a pen path is named as a feature from its first command; an edit on
            // a selection keeps its own label until a second one merges into it.
            if key == "sketch" {
                let n = 1 + t.steps.iter().filter(|s| s.feature.as_deref() == Some("sketch")).count();
                step.label = format!("{feature_label} {n}");
            } else if key == "pen" {
                step.label = feature_label.to_string();
            }
            step.feature = Some(key);
            return self.timeline_push(t, step);
        }
        self.timeline_push(t, step)
    }

    /// Append `step` to `t` and store it (see [`Session::timeline_record`]).
    fn timeline_push(&mut self, mut t: Timeline, step: Step) {
        let Some(st) = self.active_mut() else { return };
        // Rolled back: the new step goes in at the marker and the later steps replay after it.
        if let Some(m) = t.marker.take().filter(|m| *m < t.steps.len()) {
            t.steps.insert(m, step);
            let fresh = crate::History { limit: st.history.limit, byte_budget: st.history.byte_budget, ..Default::default() };
            let history = std::mem::replace(&mut st.history, fresh);
            if let Some(later) = t.steps.get_mut(m + 1..) {
                self.timeline_run(later);
            }
            let Some(st) = self.active_mut() else { return };
            st.history = history;
            store(Arc::make_mut(&mut st.doc), &t);
            st.history.recost_last(&st.doc);
            return;
        }
        t.steps.push(step);
        if t.steps.len() > MAX_STEPS {
            let extra = t.steps.len() - MAX_STEPS;
            t.steps.drain(..extra);
            t.base = None;
        }
        store(Arc::make_mut(&mut st.doc), &t);
        // The undo step's memory figures were taken against the document before the step.
        st.history.recost_last(&st.doc);
    }

    /// Run `steps` again from document `base`, as top-level commands (exactly as they first ran),
    /// each with the selection, layer and isolation it acted on. Replay stops at the first step
    /// that fails: it is marked failed with its error, the ones after it pending; the document is
    /// left as of the last good step. → the index of the failed step.
    fn timeline_replay_from(&mut self, base: Document, steps: &mut [Step]) -> Result<Option<usize>> {
        let st = self.doc_mut()?;
        st.doc = Arc::new(base);
        st.selection = Selection::default();
        st.active_layer = None;
        st.isolation = None;
        Ok(self.timeline_run(steps))
    }

    /// Run `steps` on the document as it is (see [`Session::timeline_replay_from`]).
    fn timeline_run(&mut self, steps: &mut [Step]) -> Option<usize> {
        let depth = std::mem::replace(&mut self.depth, 0);
        let replaying = std::mem::replace(&mut self.timeline_replay, true);
        // The session's template and drawing mode are the user's now, not the steps': put back.
        let template = (self.paint.clone(), self.draw_mode, self.draw_inside);
        let mut failed = None;
        // The stored steps keep the ids they were recorded with. On replay an object may be made
        // again under another id (an earlier creation is suppressed or gone): `map` takes recorded
        // ids to the ids of this run and is applied to a copy of each step as it runs, never to the
        // stored step. `gone`: what suppressed steps would have made.
        let mut map: std::collections::BTreeMap<NodeId, NodeId> = Default::default();
        let mut gone: std::collections::BTreeSet<NodeId> = Default::default();
        #[allow(clippy::needless_range_loop)] // `i` is also the failed step's index
        for i in 0..steps.len() {
            if failed.is_some() {
                steps[i].status = Status::Pending;
                steps[i].error = None;
                continue;
            }
            if steps[i].suppressed {
                steps[i].status = Status::Ok;
                steps[i].error = None;
                gone.extend(steps[i].created.iter().copied());
                continue;
            }
            // A step that works on an object a suppressed or deleted step made fails, rather than
            // act on whatever took its id. One that only had it selected and made something of
            // its own just loses that selection.
            let mut run = steps[i].clone();
            let selected_gone = run.selection.objects.iter().any(|id| gone.contains(id));
            if steps[i].orphaned || names(&steps[i], &gone) || (selected_gone && steps[i].created.is_empty()) {
                steps[i].status = Status::Failed;
                steps[i].error = Some("the objects it acted on are gone (made by a step that is suppressed or deleted)".into());
                failed = Some(i);
                continue;
            }
            if selected_gone {
                run.selection.objects.retain(|id| !gone.contains(id));
            }
            let before = self.active().map(|st| st.doc.clone());
            let was = steps[i].created.clone();
            let extend = |this: &Self, map: &mut std::collections::BTreeMap<NodeId, NodeId>| {
                if let (Some(before), Some(st)) = (before.as_ref(), this.active()) {
                    for (a, b) in was.iter().zip(created_between(before, &st.doc).iter()) {
                        if a != b {
                            map.insert(*a, *b);
                        }
                    }
                }
            };
            let mut own = run.clone();
            own.children.clear();
            remap_step(&mut own, &map);
            let mut r = self.timeline_restore(&own).and_then(|()| self.execute(&own.cmd, &own.params).map(|_| ()));
            extend(self, &mut map);
            // The feature's merged commands run after its first, in order.
            for child in &run.children {
                if r.is_err() {
                    break;
                }
                let mut child = child.clone();
                remap_step(&mut child, &map);
                r = self.timeline_restore(&child).and_then(|()| self.execute(&child.cmd, &child.params).map(|_| ()));
                extend(self, &mut map);
            }
            match r {
                Ok(()) => {
                    steps[i].status = Status::Ok;
                    steps[i].error = None;
                }
                Err(e) => {
                    steps[i].status = Status::Failed;
                    steps[i].error = Some(e.to_string());
                    failed = Some(i);
                }
            }
        }
        self.depth = depth;
        self.timeline_replay = replaying;
        (self.paint, self.draw_mode, self.draw_inside) = template;
        failed
    }

    /// Put back the selection, layer and isolation `step` acted on; an error when its objects are
    /// gone.
    fn timeline_restore(&mut self, step: &Step) -> Result<()> {
        let st = self.doc_mut()?;
        let missing: Vec<String> = step.selection.objects.iter().filter(|id| st.doc.node(**id).is_none()).map(|id| id.0.to_string()).collect();
        if !missing.is_empty() {
            return Err(EngineError::Other(format!("the objects it acted on are gone ({})", missing.join(", "))));
        }
        st.selection = step.selection.clone();
        st.active_layer = step.layer.filter(|l| st.doc.node(*l).is_some());
        st.isolation = step.isolation.filter(|l| st.doc.node(*l).is_some());
        self.paint = NewArt::defaults(step.new_art.as_ref());
        self.draw_mode = step.draw_mode;
        self.draw_inside = step.draw_inside;
        Ok(())
    }

    /// Replace the active document's steps with `steps` replayed from its base, as one undo step
    /// labelled `label`. → `{failed, error, steps}`.
    pub(crate) fn timeline_rewrite(&mut self, label: &str, edit: impl FnOnce(&mut Timeline) -> Result<()>) -> Result<Value> {
        let st = self.doc_mut()?;
        if st.interaction.is_some() {
            return Err(EngineError::Other("finish the drag first".into()));
        }
        let mut t = read(&st.doc).ok_or_else(|| EngineError::Other("this document has no timeline steps".into()))?;
        let Some(base) = t.base.as_deref() else {
            return Err(EngineError::Other(match t.base_bytes {
                Some(n) => {
                    format!("the document this timeline starts from is too large to keep ({:.1} MB), so its steps can't be edited", n as f64 / 1e6)
                }
                None => "the start of this timeline was let go (too many steps), so its steps can't be edited".into(),
            }));
        };
        let base: Document = serde_json::from_str(base).map_err(|e| EngineError::Other(format!("the timeline's base can't be read: {e}")))?;
        edit(&mut t)?;
        // Rolled back: only the steps before the marker run; the later ones wait, unrun.
        let applied = t.applied();
        let before = st.doc.clone();
        let before_sel = st.selection.clone();
        let (layer, isolation) = (st.active_layer, st.isolation);
        let fresh = crate::History { limit: st.history.limit, byte_budget: st.history.byte_budget, ..Default::default() };
        let history = std::mem::replace(&mut st.history, fresh);
        let failed = match t.steps.get_mut(..applied) {
            Some(run) => self.timeline_replay_from(base, run),
            None => Ok(None),
        };
        let st = self.doc_mut()?;
        st.history = history;
        let failed = match failed {
            Ok(f) => f,
            Err(e) => {
                st.doc = before;
                st.selection = before_sel;
                (st.active_layer, st.isolation) = (layer, isolation);
                return Err(e);
            }
        };
        store(Arc::make_mut(&mut st.doc), &t);
        st.push_undo(label.to_string(), before, Arc::new(before_sel));
        st.revision += 1;
        let error = failed.and_then(|i| t.steps.get(i)).and_then(|s| s.error.clone());
        Ok(json!({"failed": failed, "error": error, "steps": t.steps.len(), "marker": t.applied()}))
    }
}

impl Session {
    /// Change the active document's timeline without replaying it (a rename), as one undo step.
    pub(crate) fn timeline_touch(&mut self, label: &str, edit: impl FnOnce(&mut Timeline) -> Result<()>) -> Result<Value> {
        let st = self.doc_mut()?;
        let mut t = read(&st.doc).ok_or_else(|| EngineError::Other("this document has no timeline steps".into()))?;
        edit(&mut t)?;
        let before = st.doc.clone();
        let sel = st.selection.clone();
        store(Arc::make_mut(&mut st.doc), &t);
        st.push_undo(label.to_string(), before, Arc::new(sel));
        st.revision += 1;
        Ok(json!({"steps": t.steps.len(), "marker": t.applied()}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_merge_their_following_commands() {
        let sel = Selection::default();
        assert_eq!(feature_of("sketch.new", &sel), Some(("sketch".into(), true, "Sketch")));
        assert_eq!(feature_of("sketch.addLine", &sel), Some(("sketch".into(), false, "Sketch")));
        assert_eq!(feature_of("path.create", &sel).map(|f| f.1), Some(true));
        assert_eq!(feature_of("path.appendAnchor", &sel).map(|f| f.1), Some(false));
        assert_eq!(feature_of("paint.setFill", &sel), Some(("appearance:".into(), false, "Appearance")));
        assert_eq!(feature_of("shape.rectangle", &sel), None);
        assert_eq!(feature_of("object.path.offsetPath", &sel), None);
    }

    #[test]
    fn labels_name_the_command_and_its_values() {
        assert_eq!(with_values("Offset Path", &json!({"offset": 5})), "Offset Path 5");
        assert_eq!(with_values("Rectangle", &json!({"x": 1, "y": 2, "width": 50, "height": 40.25})), "Rectangle 50 × 40.25");
        assert_eq!(with_values("Move", &json!({"ids": [2]})), "Move");
    }
}
