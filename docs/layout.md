# The workbench layout file

Euclidean's Fusion layout (View > Layout > Fusion) is drawn from plain JSON data: which tabs there are, which tools, commands and panels sit in each tab's groups, and what the left column holds.

## Where the files are

- The default is compiled in from `crates/ui-egui/src/workbench/default-layout.json`.
- Your own `layout.json` sits beside `ui.json` in `~/Library/Application Support/Euclidean/`. When it exists and parses it replaces the default whole. When it exists but does not parse, the default is used and the status line says so, naming the file and the error.
- View > Layout > Reveal Layout File shows the file in the file manager (writing the default there first if it is missing). View > Layout > Reload Layout reads it again; the status line shows "Layout reloaded" or the first warning and how many more there are.
- Tab and group labels are layout data and are not translated.

## Schema

```json
{ "version": 1,
  "pinned_left": ["layers", "colour"],
  "tabs": [ { "id": "draw", "label": "DRAW",
              "groups": [ {"id": "select", "label": "SELECT", "items": ["tool:selection", "menu:Select", "sep"]} ],
              "left": ["properties"] } ],
  "nav": ["tool:hand", "tool:zoom", "cmd:view.fitArtboard", "panel:navigator"],
  "sections": { "colour": { "label": "Colour", "panels": ["color", "swatches", "stroke", "colorGuide"], "collapsed": ["colorGuide"] } },
  "ice": ["tool:touchType", "panel:actions"] }
```

- `version`: 1.
- `pinned_left`: left-column entries shown on every tab. An entry is a panel id or a section id.
- `tabs`: in order; the first is the default. Each has `id`, `label`, `groups` (each `id`, `label`, `items`) and `left` (entries below the pinned ones, again panel or section ids); and an optional `color` (one of `draw`, `sketch`, `type`, `output`: the tab's pill, group-row strip, big buttons and selected-row wash use that theme colour). A missing `color` or an unknown name uses the accent; an unknown name also shows one warning on the status line.
- `nav`: items shown with every tab (hand, zoom and so on).
- `sections`: named composite left-column sections. `panels` are the panel ids inside it; `collapsed` lists those that start folded.
- `ice`: items with no tab group, reachable elsewhere (the tool panel).

## Item grammar

- `tool:<id>`: a tool from the catalogue (`crates/tools/src/catalog.rs`).
- `cmd:<id>`: an engine command or a UI command (`menus::UI_COMMANDS`).
- `panel:<id>`: a panel id from `state::all_panels()`.
- `menu:<A>` or `menu:<A>/<B>`: every engine command whose menu path starts with that path, in registry order.
- `sep`: a separator.

## Unknown ids

An id that does not resolve is skipped with one warning ("Layout: unknown tool id 'foo' skipped"); it never stops the workbench from drawing. A `menu:` path with no commands is one warning too.

## Every tool is placed

A test (`every_catalogue_tool_is_placed_or_iced`) fails when a catalogue tool appears in no tab group, not in `nav` and not in `ice`, naming the tools. A new tool has to be placed or iced on purpose in `default-layout.json`.

## Ice

The default `ice` list holds Touch Type, Flare, the nine graph tools, and the Actions and Variables panels. Illustrator's SVG Interactivity is an iced name with no id in Euclidean, so it is not listed.

## Later

Because the layout is plain data, a parametric socket can later read or generate it (per-document or per-project layouts) without touching the drawing code.
