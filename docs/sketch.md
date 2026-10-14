# Drawing a parametric sketch (Euclidean)

A **drawn sketch** is a parametric sketch you draw the way a CAD sketcher works: draw a line
freely, type its length, Tab, type its angle, Enter. Every value you type is stored as a
**dimension**, and every dimension is a **model parameter** (`d1`, `d2`, …) that you can change
later. A constraint solver keeps the drawing consistent, so changing one dimension moves every
connected line. To the rest of the app the sketch is an ordinary group of plain paths: one closed
path per closed loop, one open path per open chain, one per circle.

Parameters in general (user parameters, expressions, favourites) are in
[parameters.md](parameters.md). B2's shape sketches (rectangle/circle/line rows, the Mounting
plate sample) still load and work. They are not converted.

## Sketch mode

- **Enter.** Object › Parametric Sketch › **New Sketch** starts an empty sketch in the current
  layer. To edit an existing drawn sketch, **double-click** it with the Selection tool.
- **While editing,** the sketch toolbar replaces the control bar at the top, in every workspace:
  - **CREATE**: Line, 2-Point Rectangle, Center Rectangle, Center Diameter Circle, 3-Point Arc,
    Fillet.
  - **MODIFY**: Fillet, Trim, Offset, Mirror, Change Parameters (`fx`).
  - **CONSTRAINTS**: Coincident, Horizontal/Vertical, Perpendicular, Parallel, Tangent, Equal,
    Fix/Unfix, Midpoint, Concentric.
  - **SKETCH DIMENSION**.
  - **FINISH SKETCH**, at the right.
  Everything outside the sketch is dimmed and cannot be selected. Sketch mode uses the app's
  isolation mode.
- **Leave.** Click **Finish Sketch**, or press Escape with no tool active. The sketch group is
  then selected.
- **Keys** (sketch mode only; outside it every shortcut is unchanged):

  | Key | Action |
  |---|---|
  | `L` | Line |
  | `R` | 2-Point Rectangle |
  | `C` | Center Diameter Circle |
  | `D` | Sketch Dimension |
| `T` | Trim (click the piece of a line to remove) |
| `O` | Offset (click a line: its whole chain; type a distance) |
  | `X` | Toggle construction on the selection |
  | Delete | Delete the selection, or the selected constraint glyph |
  | Escape | Back one step: clear the typed text, end the chain, put the tool down, finish the sketch |

  Command chords (⌘Z, ⌘S, …) work as usual. ⌘A selects the sketch's geometry.

## Heads-up input

While you draw, small boxes appear beside the geometry:

| Tool | Boxes |
|---|---|
| Line | length, angle |
| Rectangles | width, height |
| Circle | diameter |
| Arc | radius |
| Fillet | radius |

- Typing goes straight into the focused box. No click is needed.
- **Tab** locks the value and moves to the next box. A locked box keeps its value while the
  mouse moves; the others follow the mouse, and so does the preview.
- **Enter** commits. A **click** commits too, using the typed values and the mouse for the rest.
- A box takes a number (`50`), a number with a unit (`2 in`), or an expression with parameter
  names (`width / 2`). Lengths default to millimetres. Angles are degrees, measured from the
  horizontal, counter-clockwise as seen on screen.
- A box whose text does not evaluate shows the error and blocks Enter.
- Lines chain: the next segment starts at the end of the last. The chain ends on Escape, a
  double-click, or a click on its first point, which closes the loop.
- While drawing, the cursor snaps to existing sketch points (the segments then share the point)
  and to horizontal or vertical within 3°. A snap adds a horizontal or vertical constraint and
  shows its glyph.

**Only typed values become dimensions.** A value the mouse supplied creates nothing.

## Dimensions are parameters

- Each typed value becomes a dimension on the canvas (a length, an angle `30°`, a radius `R5`, a
  diameter `Ø20`) and a model parameter named `d1`, `d2`, … in creation order across the
  drawing.
- Double-click a dimension's value to change it. The solver moves every connected line so that
  all the other constraints still hold.
- **Change Parameters** (Window › Parameters, or `fx` on the sketch toolbar) lists the **User
  Parameters**, then the **Model Parameters** of each sketch, with the dimension each one
  drives. There you can rename a model parameter or give it an expression over user parameters
  (`d1 = width - 20 mm`). The star still makes a favourite.
- While a sketch is selected or being edited, the favourites appear as one slim row of sliders
  above the canvas.

## Constraints

- **Kinds:** coincident (two points, or a point on a line or circle), horizontal, vertical,
  perpendicular, parallel, tangent, equal, fix, midpoint, concentric.
- **Dimensions:** distance, length, angle (from horizontal, or between two lines), radius,
  diameter.
- **Applying.** Select the geometry, then click the constraint button. The Sketch Dimension tool
  (`D`) dimensions what you click: a line gives its length, two points their distance, a circle
  its diameter, an arc its radius, two lines the angle between them.
- **Glyphs.** Constraints show as small glyphs beside their geometry. Click a glyph and press
  Delete to remove it.
- **Colour.** Fully constrained geometry is drawn black; geometry that can still move is drawn
  blue. A new sketch has a fixed **origin point**. Tie the drawing to it (snap a line's start to
  it, or fix a point) to make the sketch fully constrained.
- **Refusal.** A constraint or dimension that would over-constrain the sketch is refused with
  "This would over-constrain the sketch", and nothing changes.
- **Construction geometry** is dashed. It takes part in constraints but is not output as paths.
- **Fillet** rounds the corner where two lines meet. The arc is tangent to both lines, its radius
  is a dimension, and the sharp corner stays as a construction point, so existing dimensions
  keep measuring to it.

The solver is [sketch-solver](https://crates.io/crates/sketch-solver) 0.1.0 (MIT OR
Apache-2.0). It works from the positions as drawn, so geometry you did not constrain stays where
you drew it.

## Commands

Every sketch action is an engine command: one undo step each, reachable from the CLI, MCP and the
control channel. Coordinates are millimetres in the sketch's frame (x right, y down, from the
sketch origin). Typed values (`length`, `angle`, `width`, `height`, `diameter`, `radius`,
`expression`) are expression strings, and each one creates a model parameter and a dimension.
`id?` defaults to the selected sketch.

| Command | Params | Result |
|---|---|---|
| `sketch.new` | `{}` | `{id}` (a new drawn sketch, selected) |
| `sketch.addLine` | `{id?, from?|fromPoint?, to?|toPoint?, length?, angle?, horizontal?, vertical?, construction?}` | `{id, line, a, b, dimensions}` |
| `sketch.addRect` | `{id?, mode?: corner|center, from, to, width?, height?}` | `{id, lines, points, dimensions}` |
| `sketch.addCircle` | `{id?, center?|centerPoint?, diameter?, r?}` | `{id, circle, center, dimensions}` |
| `sketch.addArc` | `{id?, start?|startPoint?, end?|endPoint?, through, radius?}` | `{id, arc, center, start, end, dimensions}` |
| `sketch.fillet` | `{id?, point, radius}` | `{id, arc, dimensions}` |
| `sketch.addConstraint` | `{id?, constraint: {type, …refs}}` | `{id, index}` |
| `sketch.addDimension` | `{id?, constraint: {type, …refs}, expression?}` | `{id, index, name, value}` |
| `sketch.setDimension` | `{id?, name?|index?, expression}` | `{id, name, value}` |
| `sketch.removeConstraint` | `{id?, index}` | `{id, removed}` |
| `sketch.deleteEntities` | `{id?, entities}` | `{id}` |
| `sketch.setConstruction` | `{id?, entities, construction?}` | `{id}` |
| `sketch.trim` | `{id?, entity, at: [x, y]}` (lines) | `{id}`: the piece of the line between the crossings around `at` is removed |
| `sketch.offset` | `{id?, entities, distance, side?: 1|-1}` (a chain or loop of lines) | `{id, …}`: parallel copies joined at their corners, the distance a dimension |
| `sketch.mirror` | `{id?, entities, line}` | `{id, …}`: mirrored copies with symmetric constraints |
| `sketch.inspect` (query) | `{id?}` | points, entities, constraints (expression, value and error per dimension), `dof`, `fullyConstrained` |

Constraint `type`s: `coincident`, `pointOnLine`, `pointOnCircle`, `horizontal`, `vertical`,
`parallel`, `perpendicular`, `tangent`, `equal`, `fix`, `midpoint`, `concentric`, `symmetric`.
Dimension types: `distance`, `distancePointLine`, `length`, `angle`, `angleBetween`, `radius`, `diameter`. The CLI reads a saved
drawing's parameters with
`vectorcraft-cli run --in FILE --cmd parameters.csv`; model parameters are the rows a sketch owns.

## In the file

The recipe rides on the group's `NodeExtras.sketch`: B2's `shapes` plus, for a drawn sketch,
`points`, `entities` and `constraints`, all omitted when empty. Older files load unchanged.
Parameters are in `euclidean.params`; a model parameter carries `sketch: <group id>`. The file
format version is unchanged.

Scaling or rotating a sketch with the free-transform tools changes its paths but not its recipe.
The next rebuild restores the recipe's geometry (as in B2).
