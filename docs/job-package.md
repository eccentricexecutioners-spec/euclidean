# Job package (`euclidean-job/1`)

*Status: written for Euclidean 0.1.5 (E4 Part D), 2026-10-17. Nothing in it is uploaded: the package is a folder on disk.*

A **job** is what leaves Euclidean for a laser shop: every sheet of a nest, in the formats a shop's
software opens, and one `job.json` that says what is in the folder. Sheet & Nest › **Export Job…**
(or File › Export › Export Job…, or the command `nest.exportJob {path}`) writes it.

## The folder

For each sheet (an artboard with sheet settings; see Sheet & Nest), named after the sheet:

| File | What it is |
|---|---|
| `Sheet 1.svg` | The sheet at true size: `width="…mm" height="…mm"`, the drawing in points in the `viewBox`. LightBurn, Inkscape and most cutter software open it at the right size. |
| `Sheet 1.dxf` | The same in DXF, millimetres (`$INSUNITS` 4), 1:1. |
| `Sheet 1.lbrn2` | A LightBurn project: millimetres, one cut layer per colour (see below). |
| `Sheet 1.png` | A thumbnail of the sheet, at most 512 px on its longer side. |

and `job.json`.

## `job.json`, field by field

```json
{
  "schema": "euclidean-job/1",
  "id": "3f2b9c4e-8a1d-4f6b-9c2e-5d7a1b3c4e5f",
  "created": "2026-10-17T18:30:00Z",
  "app": { "name": "Euclidean", "version": "0.1.5" },
  "document": "plates.vectorcraft",
  "units": "mm",
  "sheets": [
    {
      "name": "Sheet 1",
      "artboard": 1,
      "material": "Plywood",
      "width_mm": 400.0,
      "height_mm": 300.0,
      "thickness_mm": 6.0,
      "part_count": 3,
      "files": ["Sheet 1.svg", "Sheet 1.dxf", "Sheet 1.lbrn2"],
      "thumbnail": "Sheet 1.png"
    }
  ],
  "operations": [
    { "colour": "#000000", "kind": "cut", "lightburn_layer": 0 }
  ],
  "order": { "account": null, "link_code": null, "shop": null }
}
```

| Field | Type | Meaning |
|---|---|---|
| `schema` | string | Always `euclidean-job/1` for this layout. A reader refuses a schema it does not know. |
| `id` | string | A version-4-shaped UUID that tells jobs apart. Made from the clock, the process and the document (Euclidean links no random source); not a secret. |
| `created` | string | When the job was written, UTC, ISO 8601 (`YYYY-MM-DDThh:mm:ssZ`). Empty on a platform without a clock (the web build). |
| `app.name`, `app.version` | string | The writer: `Euclidean` and its version. |
| `document` | string | The document's file name, or `Untitled` when it has none. |
| `units` | string | Always `mm`: every length in the file is in millimetres. |
| `sheets[]` | array | One entry per sheet, in artboard order. |
| `sheets[].name` | string | The sheet's artboard name ("Sheet 1"). |
| `sheets[].artboard` | integer | Its artboard index in the document (0-based). |
| `sheets[].material` | string or null | The material named in Sheet & Nest ("Plywood", "MDF", "Acrylic", or what was typed); null when none was given. |
| `sheets[].width_mm`, `height_mm` | number | The sheet's size (the artboard's), to a thousandth of a millimetre. |
| `sheets[].thickness_mm` | number or null | The material thickness; null when not given. |
| `sheets[].part_count` | integer | The objects wholly on the sheet. |
| `sheets[].files` | array of string | The sheet's files in this folder: SVG, DXF and LightBurn. |
| `sheets[].thumbnail` | string | The sheet's PNG in this folder. |
| `operations[]` | array | One entry per colour used on any sheet. |
| `operations[].colour` | string | The colour as `#rrggbb`. |
| `operations[].kind` | string | `cut` (stroked art) or `engrave` (filled art without a stroke). `score` is part of the schema for the shop to choose; Euclidean does not pick it. |
| `operations[].lightburn_layer` | integer | The LightBurn layer (0–29) the colour went to in the `.lbrn2` files. |
| `order` | object | Filled when the job is ordered; all null when Euclidean writes it. |
| `order.account` | string or null | The account billed. |
| `order.link_code` | string or null | The link code of the Euclidean install that sent the job. |
| `order.shop` | string or null | The shop that cuts it (MakerLabs, Dependable…). |

Every file a `sheets[]` entry names is in the folder, and the folder holds nothing else but `job.json`
(the engine test `sheet_output::a_two_sheet_job_folder_holds_every_file_job_json_names_and_job_json_round_trips`).

## Ordering, later

Euclidean writes the `order` block empty. When ordering is built, **the Mothership attaches the
account from the install's link code**: billing is per link code (Karl's ruling, 2026-10-17). The
app will send the folder with its link code; the Mothership looks the code up, fills `order.account`,
`order.link_code` and `order.shop`, and hands the job to the shop's machine (through Sieglinde).
Nothing in this version talks to a network.

## The LightBurn file

LightBurn does not publish its project format. The writer (`crates/engine/src/cmd/fileio/lbrn.rs`)
follows files LightBurn itself saved and these MIT-licensed projects, read only, nothing copied:
[jlucaso1/lbrn2-to-svg](https://github.com/jlucaso1/lbrn2-to-svg),
[styx/lac_to_lbrn2](https://github.com/styx/lac_to_lbrn2),
[nuCarve/laserpost](https://github.com/nuCarve/laserpost). The layer colours are LightBurn's
documented palette ([Layer Colors](https://docs.lightburnsoftware.com/legacy/LayerColors)).

- Millimetres, Y up from the sheet's bottom-left corner (checked against a LightBurn-saved file's own
  thumbnail): a W × H mm sheet spans 0…W, 0…H.
- One `CutSetting` per layer used, `type="Cut"` (Line mode), LightBurn's default power and speed:
  power and speed are set at the laser.
- Each document colour goes to the **nearest free** LightBurn layer (C00 black, C01 blue, C02 red,
  C03 green…): two colours never share a layer. More than 30 colours is refused.
- Paths are `Path` shapes with lines (`L`) and cubic curves (`B`). Type, placed images, symbols and
  live effects are left out with a note: outline or expand them first.
- `AppVersion` is written as `1.0.00` (an early LightBurn version, so any LightBurn opens it as an
  older file); the writer's own name and version are in the file's Notes.

Opening a `.lbrn2` (File › Open in LightBurn) brings its layers; **importing** it brings the shapes
only. NOT MEASURED in Euclidean: a real LightBurn opening the file (Karl tries it on his laser).
