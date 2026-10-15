# The timeline (Euclidean)

The timeline is the document's command steps, in order, and it can be edited. Double-click a step on
the timeline strip (or run `timeline.editStep`) to change its parameters. The document goes back to just
before that step, the step runs with the new parameters, and every later step replays after it, as
Fusion's timeline does. The whole edit is one undo step.

Code: `crates/engine/src/timeline.rs` (model, recording, replay), `crates/engine/src/cmd/timeline.rs`
(commands), `crates/engine/tests/timeline.rs` (tests). The strip is drawn by
`crates/ui-egui/src/workbench/` in the Fusion layout.

## What a step is

Each of these appends one step, as long as it changes the active document:

- a journaled top-level command (`Session::execute`'s journal push);
- a committed drag (`Session::commit_interaction`, which journals once, with its last preview).

These are not steps:

- query commands (`journal: false`);
- commands that change only the selection, the view or panel state. A selection is recorded as part of
  the step that acts on it, never as a step of its own;
- `edit.undo` and `edit.redo` (and `edit.undoTo`, `edit.redoTo`, `file.revert`), which move between
  documents that already hold their own steps;
- the timeline's own commands.

Undo removes the last step and redo restores it with no code of their own, because the undo history
keeps whole documents and the timeline is part of the document.

## Where it is stored

The timeline lives inside the document, in `document.unknown["euclidean.timeline"]`, the same way B2
stores `euclidean.params`. There is no file-format version change. Its stored form is plain JSON:

```json
{
  "version": 1,
  "base": "{…the document before the first step, as JSON text…}",
  "base_bytes": null,
  "steps": [
    {
      "cmd": "shape.rectangle",
      "params": {"x": 10, "y": 10, "width": 50, "height": 40},
      "label": "Rectangle 40 × 50",
      "selection": {"objects": []},
      "layer": 1,
      "new_art": {"fill": …, "stroke": …, "stroke_width": 1, "opacity": 1, …},
      "draw_mode": "normal",
      "draw_inside": null,
      "status": "ok",
      "error": null
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `base` | The document as it was before the first step, serialized with `serde_json` and stored as one string, so recording a step copies it as one block of bytes. A new document's base is the empty document (about 1–2 KB). A document opened from a file starts from the document as opened. |
| `base_bytes` | Set instead of `base` when the base is larger than `BASE_CAP` (1 MB of JSON). The steps are still recorded and listed, but they can't be edited. Every step copies the base and the undo history keeps one copy per step, so a large base is not kept. |
| `steps[].cmd`, `params` | The command id and its params as the journal records them, with the values the command noted (`note_journal`: a date, a preference it read). |
| `steps[].label` | What the strip shows: the command's label and its first two plain values. |
| `steps[].selection` | The selection the step acted on (a `vectorcraft_doc::Selection`: objects by `NodeId`, plus direct-selected anchors and segments, key object, target). |
| `steps[].layer`, `isolation` | The current layer and the isolated group the step acted in. |
| `steps[].new_art`, `draw_mode`, `draw_inside` | The new-art template (fill, stroke, appearance; omitted when it is the default white fill and 1 pt black stroke) and the drawing mode. Set Fill with nothing selected changes the template, not the document, and new art takes it. |
| `marker` | Present only when the end marker is rolled back: how many steps are applied. The later steps are kept but not run, and they are greyed on the strip. |
| `steps[].status`, `error` | Omitted when `ok`. `failed` is the step where the last replay stopped, with its error. `pending` marks a step after it that was kept but not run. |

Most steps kept: 2000. Past that, the oldest go and the base is let go, so editing is off.

Saving and loading keep everything, because the timeline rides in the native file's `unknown` map.
Formats that don't keep `unknown` (SVG, PDF, EPS exports) don't carry it.

## Replay

`timeline.editStep` and `timeline.replay` do the following:

1. Put the document back to the base.
2. Run every step as a top-level command, exactly as it first ran. Before each step, its selection,
   layer, isolation, new-art template and drawing mode are restored.
3. Stop at the first step that fails, either because the objects it acted on are gone or because the
   command returned an error. That step is marked `failed` with its error, the steps after it are
   marked `pending` (kept, unrun), and the document is left as of the last good step. Undo returns to
   before the edit.
4. Afterwards, the session's own new-art template and drawing mode are put back. They belong to the
   user's next action, not to the steps.

Measured determinism (C1 Step 1 and `replaying_unchanged_steps_gives_the_same_document`): replaying
unchanged steps from the base gives a byte-identical document (serde JSON). `NodeId`s are allocated
deterministically, and the file format rebuilds the allocator from the largest id when a file is loaded.

## The end marker

`timeline.rollTo {index}` (or dragging the end marker on the strip) replays the first `index` steps from
the base. The steps after the marker are kept, unrun, and greyed. A new action taken while the
timeline is rolled back is **inserted at the marker**, and the later steps replay after it, as in
Fusion. The insert and the replay together are one undo step. While rolled back, an edited step
replays only up to the marker.

## Commands

| Id | Kind | Params → result |
|---|---|---|
| `timeline.list` | query | `{}` → `{steps: [{index, cmd, label, params, selection: [ids], status, error, rolledBack}], marker, editable, baseBytes}` |
| `timeline.editStep` | journaled, one undo step | `{index, params}`: `params` is merged over the step's params (`null` removes a key) → `{failed: index or null, error, steps}` |
| `timeline.replay` | journaled, one undo step | `{}` replays every step from the base, so a failed step is tried again → as above |
| `timeline.rollTo` | journaled, one undo step | `{index}`: how many steps stay applied (0 … steps). The document shows the timeline as of that step, and the later steps are kept but not run → `{failed, error, steps, marker}` |
| `timeline.setRecording` | query | `{on}`: records steps from now on. The app records; headless sessions (CLI, MCP, tests) start without recording → `{on}` |

The commands reach the CLI (`vectorcraft-cli run --cmd timeline.list`), MCP `command_run` and the
control channel (`engine.execute`) like every other command.

## Later: the parametric socket

The timeline is plain JSON in the document, and its commands are ordinary registry commands. Another EE
app can read the steps (`timeline.list`) and edit a step (`timeline.editStep`) through the same channels,
with no format of its own.
