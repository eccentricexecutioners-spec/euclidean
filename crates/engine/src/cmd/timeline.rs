//! The timeline's commands (Euclidean, [`crate::timeline`]): list the steps, edit one and replay
//! the ones after it, roll the end marker back. The timeline strip, the CLI (`vectorcraft-cli run
//! --cmd timeline.list`), MCP `command_run` and the control channel all reach the timeline here.
use super::*;
use crate::timeline::{self, Status};
use serde_json::{Value, json};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "timeline.list",
            "List Timeline Steps",
            [],
            None,
            "{} → {steps: [{index, cmd, label, name, feature, children: [{cmd, params}], params, selection: [ids], status: ok|failed|pending, error, rolledBack, suppressed}], marker (steps applied), editable (the base is kept), baseBytes} the active document's timeline, oldest step first; a feature step (a sketch, a pen path) lists the commands merged into it as children",
            has_doc,
            list
        ),
        cmd!(
            query "timeline.setRecording",
            "Record Timeline",
            [],
            None,
            "{on: bool} record a timeline step for every edit from now on (the app records; headless sessions start without) → {on}",
            always,
            set_recording
        ),
        cmd!(
            "timeline.editStep",
            "Edit Timeline Step",
            [],
            None,
            "{index, params (merged over the step's params; null removes a key)} the document goes back to just before the step, runs it with the new params, then replays every later step; replay stops at a step that fails (marked failed, the later ones pending). One undo step → {failed: index|null, error, steps}",
            has_timeline,
            edit_step
        ),
        cmd!(
            "timeline.rollTo",
            "Roll Timeline",
            [],
            None,
            "{index (how many steps stay applied, 0…steps)} moves the end marker: the document shows the timeline as of that step, the later steps kept, unrun (greyed); a new action is inserted at the marker and the later steps replay after it. One undo step → {failed: index|null, error, steps, marker}",
            has_timeline,
            roll_to
        ),
        cmd!(
            "timeline.suppressStep",
            "Suppress Timeline Step",
            [],
            None,
            "{index, on} skips the step (on) or runs it again (off) on replay; the later steps replay without it, and one that then fails is marked failed. One undo step → {failed: index|null, error, steps, marker}",
            has_timeline,
            suppress_step
        ),
        cmd!(
            "timeline.deleteStep",
            "Delete Timeline Step",
            [],
            None,
            "{index} removes the step and replays the later ones without it (they are renumbered); one that then fails is marked failed. One undo step → {failed: index|null, error, steps, marker}",
            has_timeline,
            delete_step
        ),
        cmd!(
            "timeline.renameStep",
            "Rename Timeline Step",
            [],
            None,
            "{index, name | null} names the step (shown instead of its label; null takes the name away). One undo step, no replay → {steps, marker}",
            has_timeline,
            rename_step
        ),
        cmd!(
            "timeline.replay",
            "Replay Timeline",
            [],
            None,
            "{} runs every step again from the timeline's base (a failed step is tried again). One undo step → {failed: index|null, error, steps}",
            has_timeline,
            replay
        ),
    ]
}

fn has_timeline(s: &Session) -> std::result::Result<(), String> {
    let st = s.active().ok_or("no document open")?;
    if timeline::read(&st.doc).is_some_and(|t| !t.steps.is_empty()) { Ok(()) } else { Err("this document has no timeline steps".into()) }
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let st = s.doc()?;
    let t = timeline::read(&st.doc).unwrap_or_default();
    let applied = t.applied();
    let steps: Vec<Value> = t
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            json!({
                "index": i,
                "cmd": step.cmd,
                "label": step.shown(),
                "name": step.name,
                "feature": step.feature,
                "children": step.children.iter().map(|c| json!({"cmd": c.cmd, "params": c.params})).collect::<Vec<_>>(),
                "params": step.params,
                "selection": step.selection.objects,
                "status": step.status,
                "error": step.error,
                "rolledBack": i >= applied,
                "suppressed": step.suppressed,
                "created": step.created,
                "orphaned": step.orphaned,
            })
        })
        .collect();
    Ok(json!({"steps": steps, "marker": applied, "editable": t.base.is_some(), "baseBytes": t.base.as_ref().map_or(t.base_bytes, |b| Some(b.len()))}))
}

fn set_recording(s: &mut Session, p: &Value) -> Result<Value> {
    let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad("timeline.setRecording", "`on` must be true or false"))?;
    s.timeline_recording = on;
    Ok(json!({"on": on}))
}

fn edit_step(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.editStep";
    let index = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`index` must be a step number (0 = the first step)"))?;
    let Some(Value::Object(patch)) = p.get("params") else { return Err(bad(cmd, "`params` must be an object")) };
    let patch = patch.clone();
    s.timeline_rewrite("Edit Timeline Step", |t| {
        let n = t.steps.len();
        let step = usize::try_from(index)
            .ok()
            .and_then(|i| t.steps.get_mut(i))
            .ok_or_else(|| bad(cmd, format!("no step {index} (the timeline has {n})")))?;
        let mut params = match &step.params {
            Value::Object(o) => o.clone(),
            _ => serde_json::Map::new(),
        };
        for (k, v) in patch {
            if v.is_null() {
                params.remove(&k);
            } else {
                params.insert(k, v);
            }
        }
        step.params = Value::Object(params);
        step.label = timeline::label_for(&step.cmd, &step.params);
        step.status = Status::Ok;
        Ok(())
    })
}

fn roll_to(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.rollTo";
    let index = p
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .ok_or_else(|| bad(cmd, "`index` must be a step count (0 = before the first step)"))?;
    s.timeline_rewrite("Roll Timeline", |t| {
        let n = t.steps.len();
        if index > n {
            return Err(bad(cmd, format!("the timeline has {n} steps")));
        }
        t.marker = (index < n).then_some(index);
        Ok(())
    })
}

fn suppress_step(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.suppressStep";
    let index = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`index` must be a step number (0 = the first step)"))?;
    let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad(cmd, "`on` must be true or false"))?;
    let label = if on { "Suppress Timeline Step" } else { "Unsuppress Timeline Step" };
    s.timeline_rewrite(label, |t| {
        let n = t.steps.len();
        let step = usize::try_from(index)
            .ok()
            .and_then(|i| t.steps.get_mut(i))
            .ok_or_else(|| bad(cmd, format!("no step {index} (the timeline has {n})")))?;
        step.suppressed = on;
        Ok(())
    })
}

fn delete_step(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.deleteStep";
    let index = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`index` must be a step number (0 = the first step)"))?;
    s.timeline_rewrite("Delete Timeline Step", |t| {
        let n = t.steps.len();
        let i = usize::try_from(index).ok().filter(|i| *i < n).ok_or_else(|| bad(cmd, format!("no step {index} (the timeline has {n})")))?;
        // Steps that acted on what this one made are orphaned: they fail rather than act on
        // whatever object takes those ids.
        let gone: std::collections::BTreeSet<_> = t.steps[i].created.iter().copied().collect();
        for later in t.steps.iter_mut().skip(i + 1) {
            if timeline::references(later, &gone) {
                later.orphaned = true;
            }
        }
        t.steps.remove(i);
        if let Some(m) = t.marker {
            let m = if i < m { m - 1 } else { m };
            t.marker = (m < t.steps.len()).then_some(m);
        }
        Ok(())
    })
}

fn rename_step(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "timeline.renameStep";
    let index = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "`index` must be a step number (0 = the first step)"))?;
    let name = match p.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(n)) if n.trim().is_empty() => None,
        Some(Value::String(n)) => Some(n.trim().chars().take(80).collect::<String>()),
        Some(_) => return Err(bad(cmd, "`name` must be text or null")),
    };
    s.timeline_touch("Rename Timeline Step", |t| {
        let n = t.steps.len();
        let step = usize::try_from(index)
            .ok()
            .and_then(|i| t.steps.get_mut(i))
            .ok_or_else(|| bad(cmd, format!("no step {index} (the timeline has {n})")))?;
        step.name = name;
        Ok(())
    })
}

fn replay(s: &mut Session, _: &Value) -> Result<Value> {
    s.timeline_rewrite("Replay Timeline", |_| Ok(()))
}
