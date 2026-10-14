# User parameters and parametric sketches (Euclidean)

A **user parameter** is a named value saved in the drawing: `width = 120 mm`, `holes = 4`,
`slot_w = width / 2`. A **parametric sketch** is a group of plain paths (rectangles, circles,
lines) whose every size and position is an expression over those parameters. Change a parameter
and every sketch that uses it is rebuilt, in the same undo step.

Window › **Parameters** shows the parameters (favourites with a slider across the top, every
row below, the selected sketch's shapes at the bottom). Object › **Parametric Sketch** adds a
sketch (New Rectangle, New Circle, New Line) or the **Sample: Mounting plate**. While one sketch
is selected, the control bar shows the favourite parameters as sliders.

Everything below is an engine command, so the panel, the CLI (`vectorcraft-cli run --cmd …`),
MCP (`command_run`) and the control channel do the same thing.

## Parameters

A row is `{name, unit, expression, comment, favourite, min, max, step}`.

- **name**: a letter or `_`, then letters, digits and `_`; unique; not a function or unit name.
- **unit**: `mm`, `in`, or `""` (no unit: a count or a ratio). Values are shown and slider
  ranges are written in the row's unit.
- **expression**: numbers with an optional unit (`mm cm m in ft pt`, lengths evaluate to
  millimetres), `+ - * / ^`, unary minus, parentheses, `sqrt abs round floor ceil min max sin
  cos tan` (degrees), `pi`, and other parameters' names. A bare number is a plain number.
- **favourite**: starred rows get a slider (when the expression is a plain number) in the panel
  and in the control bar; a starred formula shows its value only. Starring a row without a range
  gives it `min` = half and `max` = double its value, stepping by 1 (no unit), 0.5 (mm) or 0.125 (in).

Errors are plain text: `no parameter "x"`, `x loops back to itself`, `divide by zero`,
`unexpected "#"`, `unfinished`. A row that does not evaluate shows **error** (hover for the text);
a sketch shape whose fields do not all evaluate keeps its previous paths.

| Command | Params | Result |
|---|---|---|
| `parameters.list` (query) | `{}` | `{rows: [{name, unit, expression, value, error, comment, favourite, min, max, step}]}`; `value` in the row's unit |
| `parameters.csv` (query) | `{}` | `{csv}`: `name,unit,expression,comment` rows under a header (the HBC Client Params form) |
| `parameters.add` | `{name?, unit?, expression?, comment?, favourite?, min?, max?, step?}` | `{name}` (default `param1`, `param2`…) |
| `parameters.set` | `{name, expression?, unit?, comment?, favourite?, min?, max?, step?}` (`null` clears a range value) | `{name, value, error}`; one undo step |
| `parameters.remove` | `{name}` | `{name}` |
| `parameters.rename` | `{from, to}` | rewrites every whole-word reference in the other rows and in every sketch field |

The rows live in `document.unknown["euclidean.params"]`, so the file format is unchanged and
every exporter ignores them. Headless: `vectorcraft-cli run plate.vectorcraft --cmd parameters.list`.

## Sketches

A sketch is an ordinary `Group` whose `sketch` field holds the recipe: `{origin: [x, y] (mm),
shapes: [{id, kind, fields}]}`. Field expressions are in millimetres; y runs down the page from
the artboard's top-left corner, as every document coordinate does.

| Kind | Fields |
|---|---|
| `rect` | `x, y, w, h, r` (corner radius; 0 allowed) |
| `circle` | `cx, cy, d, n, pitch` (`n` copies in a row along x, `pitch` apart; `n` defaults to 1, at most 200) |
| `line` | `x, y, len, ang` (degrees, counter-clockwise on the page) |

The group's children are plain paths (1 pt black stroke, no fill), one per shape copy, named
after their shape (`R1`, `C1`…). They are regenerated whenever a parameter or the recipe changes:
their ids may change. Moving a sketch with the Selection tool moves its `origin`; scaling or
rotating it leaves the paths transformed until the next rebuild draws the recipe again.

| Command | Params | Result |
|---|---|---|
| `sketch.create` | `{shapes?: [{kind, id?, fields?}]}` (default: one 40 × 30 mm rectangle) | `{id}`, selected, in the current layer |
| `sketch.sample` | `{}` | the sample's seven parameters (those the drawing lacks) and its sketch → `{id}` |
| `sketch.addShape` | `{id?, kind}` (`id` defaults to the selected sketch) | `{id, shape}` |
| `sketch.setField` | `{id?, shape, field, expression}` | `{id, shape, field, value, error}` |
| `sketch.removeShape` | `{id?, shape}` | `{id, shape}` |
| `sketch.inspect` (query) | `{id?}` | `{id, origin, shapes: [{id, kind, fields: {field: {expression, value, error}}, error}]}` |

### The sample

Parameters `width 120 mm ★ (60–300, 1)`, `height 80 mm ★ (40–200, 1)`, `margin 12 mm`,
`hole_dia 8 mm ★ (3–20, 0.5)`, `holes 4 ★ (no unit, 2–10, 1)`, `corner 6 mm`, `slot_w = width / 2`.
Shapes `R1 rect(0, 0, width, height, corner)`, `C1 circle(margin, margin, hole_dia, holes,
(width - 2*margin) / (holes - 1))`, `C2` the same row at `height - margin`, `R2 rect((width -
slot_w) / 2, height/2 - 5 mm, slot_w, 10 mm, 5 mm)`.

Not here yet: a Dimension tool or dimension lines driving a sketch (planned on top of upstream's
Dimension tool), a constraint solver, turning drawn art into a sketch.
