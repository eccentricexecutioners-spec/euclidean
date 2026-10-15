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
            "{} → {steps: [{index, cmd, label, params, selection: [ids], status: ok|failed|pending, error}], editable (the base is kept), baseBytes} the active document's timeline, oldest step first",
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
    let steps: Vec<Value> = t
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            json!({
                "index": i,
                "cmd": step.cmd,
                "label": step.label,
                "params": step.params,
                "selection": step.selection.objects,
                "status": step.status,
                "error": step.error,
            })
        })
        .collect();
    Ok(json!({"steps": steps, "editable": t.base.is_some(), "baseBytes": t.base.as_ref().map_or(t.base_bytes, |b| Some(b.len()))}))
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
    s.timeline_rewrite("Edit Timeline Step", |steps| {
        let n = steps.len();
        let step =
            usize::try_from(index).ok().and_then(|i| steps.get_mut(i)).ok_or_else(|| bad(cmd, format!("no step {index} (the timeline has {n})")))?;
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

fn replay(s: &mut Session, _: &Value) -> Result<Value> {
    s.timeline_rewrite("Replay Timeline", |_| Ok(()))
}
