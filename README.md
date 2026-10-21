# OpenCheck

OpenCheck is an open source BIM model checker written in Rust. It loads several
IFC files into one federated model, lets you browse and inspect them in an
easy 3D viewer, and runs clash detection between them using configurable
rulesets.

> Status: early development. Model viewing, measuring, projects and clash
> checking with rulesets work. Issues, reports and BCF are next; see
> [docs/ROADMAP.md](docs/ROADMAP.md).

## Goals

- **Model**: load multiple IFC files (IFC2x3, IFC4, IFC4x3), switch models on
  and off, browse the spatial structure and read properties of any element.
- **Viewer**: navigation that needs no training. Orbit around the point under
  the cursor, zoom to an element with a double click, filter by storey, section
  box, isolate and X-ray.
- **Checking**: clash detection between models and disciplines, driven by
  rulesets stored as plain text files that can live in version control.

## Workspace layout

| Crate | Purpose |
|---|---|
| `opencheck-core` | IFC loading, element and property model, spatial tree, federation |
| `opencheck-cli` | The `opencheck` command line tool |
| `opencheck-app` | Desktop application: federated 3D viewer (egui + wgpu) |

IFC parsing and tessellation are provided by
[ifc-lite](https://github.com/LTplus-AG/ifc-lite) (MPL-2.0). OpenCheck wraps it
in its own types in `opencheck-core`, so the rest of the code does not depend on
the ifc-lite API.

## Building

Install Rust from <https://rustup.rs>, then:

```sh
cargo build --release
```

On Windows either the MSVC toolchain (with Visual Studio Build Tools) or the
GNU toolchain works. The GNU toolchain additionally needs a MinGW-w64
installation with binutils on `PATH`, for example WinLibs.

## Trying it out

Download the buildingSMART sample models and inspect them:

```sh
sh scripts/fetch-testdata.sh
cargo run -p opencheck-cli -- info testdata/ifc4/Building-*.ifc
cargo run -p opencheck-cli -- tree testdata/ifc4/Building-Architecture.ifc --elements
cargo run -p opencheck-cli -- props testdata/ifc4/Building-Architecture.ifc 1AQAupaRP1txwK1AGiN61V
```

Start the viewer, optionally with files to open:

```sh
cargo run -p opencheck-app -- testdata/ifc4/Building-*.ifc
```

| Input | Action |
|---|---|
| Left drag | Orbit around the point under the cursor |
| Right or middle drag | Pan |
| Scroll | Zoom towards the cursor |
| Click / Ctrl+click | Select / add to selection |
| Double click | Zoom to element |
| W A S D, Q E | Walk, down / up (Shift = faster) |
| F, Home | Zoom to selection, zoom to everything |
| H, Shift+H, I | Hide selection, show all, isolate |
| X | X-ray |
| C | Section just above the selection; drag the scissors to move it |
| M | Laser measure (click keeps a reading). Ctrl+click measures point to point and snaps to corners and edges |
| Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S | New, open, save, save as project |

## Checking

The Checking tab runs rulesets against the loaded models, much like Solibri:

- Every project checks for **duplicates**: elements of one model with the same
  shape lying exactly on top of each other, whatever their class or
  tessellation. This standard ruleset can be switched off but not removed.
- Add built-in rulesets (MEP vs structure, MEP vs architecture, structure vs
  architecture, MEP coordination, clearance) or import your own.
- Choose per ruleset which models it runs on.
- The **Ruleset Manager** creates and edits rulesets and rules in forms
  (check type, severity, tolerance, groups A and B by discipline, IFC class
  and property), shows how many elements each group matches, keeps a personal
  library of rulesets for other projects, and runs checks. Classes, property
  sets, properties and values are picked from what the loaded models contain;
  "Try the rule" explains why the selected element is in a group or not, and
  "Show matches in 3D" highlights everything a rule takes.
- Results are grouped by rule, category (pair of IFC classes) and component.
  Selecting a finding shows its two elements in orange and blue with the rest
  ghosted. Accept real problems and reject the rest; decisions are kept
  between runs.
- Ctrl+click, Shift+click or right-click a group to select several findings
  and add them as **issues**. Issues follow the BCF topic model and are the
  material for reports.
- **Information requirements**: building parts must carry the information
  their level of development requires (LOD 200, 300, 325 or 400 DK). The
  level is set for the project on the ruleset card and can be changed per
  model in the model list. Results show the fill rate per building part and
  every element with what it is missing. Classification and other values are
  found wherever the project keeps them; see
  [docs/information-requirements.md](docs/information-requirements.md).
- **Responsibility and BCF Live (Dalux)**: every issue names the
  disciplines responsible for it (by default those of the clashing models)
  and can be assigned to a person. Issues are sent to a Dalux project as BCF
  topics with their viewpoint; status and assignee changes made in Dalux and
  comments come back. See [docs/bcf-live.md](docs/bcf-live.md).
- **Criticality and exports**: issues are rated C1 (not critical), C2
  (critical) or C3 (very critical) as Danish practice for consistency and
  clash checks recommends, and are assigned by initials (initials used once
  are offered again). *Export* in the Issues tab writes the ticked issues (or
  all) as a **BCF 2.1 file** (`.bcfzip`, with pictures and viewpoints) or as an
  illustrated **report** (one HTML file in Danish, printable to PDF) with
  introduction, models, software and every issue with picture, criticality,
  responsible disciplines and the IFC GlobalIds of its objects.
- **Working area (Edit view)**: at the bottom of the model list, *Edit view*
  switches to a top view where you drag a rectangle around the area you
  want; drag inside to move it, drag its edges to resize, and *Align to
  element* turns the view and the rectangle along an element such as a wall.
  After *Save* only elements touching the area are shown and checked, so
  clashes can be run for one area. *Reset view* brings everything back. The
  IFC files are not changed.
- Models change during a project: the update button on a model card loads a
  newer version of that IFC file. The check runs again, and clashes in issues
  that are gone are marked **Solved by update** instead of being deleted.

Rulesets are TOML files; [docs/ruleset-authoring.md](docs/ruleset-authoring.md)
describes the format and can be handed to a language model to write custom
rulesets (the app offers it as a download). From the command line:

```sh
cargo run -p opencheck-cli -- rulesets
cargo run -p opencheck-cli -- check testdata/ifc4/Building-*.ifc --all
```

## Projects

A project is saved as one `.opencheck` file: a ZIP archive holding the IFC
files unchanged, the rulesets and the view state. See
[docs/file-format.md](docs/file-format.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Dependencies carry their own licenses; ifc-lite is MPL-2.0.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
