# OpenCheck

OpenCheck is an open source BIM model checker written in Rust. It loads several
IFC files into one federated model, lets you browse and inspect them in an
easy 3D viewer, checks them for clashes, clearances, free zones, duplicates
and missing information, and turns what it finds into issues that can be
reported, exported as BCF or shared with an issue server such as Dalux.
Visual checks colour the model to show what is out of line: walls and
columns that do not stack, pipes and ducts that grow again away from their
source, drains without fall, elements on the wrong storey. Checks can also
run on a schedule without the app, with a dashboard of how a project
develops.

> Status: early but usable. Viewing, checking (including free zones and
> script rules), visual checks, information requirements, IDS, issues,
> reports, BCF files, BCF Live with Dalux and autorun work, on the desktop
> and largely in the browser. See [docs/ROADMAP.md](docs/ROADMAP.md) for what comes next.

## Download

You do not need to build OpenCheck to try it. For Windows:

1. Open the [Releases page](https://github.com/sparboom-git/opencheck/releases)
   and download `OpenCheck-<version>-windows-x64.zip` from the latest release.
2. Unzip it anywhere, for example on your desktop. Nothing is installed.
3. Double-click `opencheck-app.exe` to start the viewer. `opencheck.exe` is the
   command line tool.

The program is not code-signed, so Windows SmartScreen may show "Windows
protected your PC" the first time. Click **More info** and then **Run anyway**.

### In the browser

OpenCheck also runs in the browser, without installing anything:
<https://sparboom-git.github.io/opencheck/>. It needs WebGPU, which recent
Chrome and Edge have. IFC files are opened in the browser and never uploaded.

The browser version is new and does a little less than the desktop program.
Models are opened (picked or dropped on the page), viewed and checked; issues,
reports and BCF files are downloaded. *Save* downloads the project as an
`.opencheck` file and *Open project* (or dropping the file on the page) opens
it again, so projects move freely between the browser and the desktop
program. A note on the first visit says what the web version is; it can be
turned off. Not in the browser: updating a model in place, BCF Live, autorun,
dialogs in windows of their own, and remembering the theme. Large models load
slower, and the page waits while a model loads or a check runs.

## What it does

### Model

- Load several IFC files (IFC2x3, IFC4, IFC4x3) into one federated model;
  drop files on the window or use *File → Add IFC models*. The discipline of
  each model is guessed from its file name and can be changed.
- Switch models on and off, browse the spatial structure and read the
  properties, classifications, materials and systems of any element.
- **Update a model** to a newer version of its IFC file with the ↻ button on
  its card. Checks run again, and clashes in issues that disappear are marked
  **Solved by update** instead of being deleted.
- **Working area** (*Edit view* at the bottom of the model list): drag a
  rectangle in a top view around the area you want; drag inside to move it,
  drag its edges to resize, and *Align to element* turns the view and the
  rectangle along an element such as a wall. In the *Side view* the heights
  can be limited too: drag the bottom and top lines (they snap to the
  storeys), or choose *From* one storey *to* another, e.g. to check only two
  floors. After *Save*, only elements touching the area are shown and
  checked. *Reset view* brings everything back. The IFC files are not
  changed.

### Viewer

Navigation that needs no training:

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
| F12 | Frame time and display scale in the status bar |
| Walk button (person) | Click a floor to stand there, then walk at eye height: W A S D, drag to look around, scroll to step, Esc stops |
| Right-click in the model tree | Isolate, hide, show, select or zoom to a storey, a class (e.g. all beams) or an element |

### Checking

The Check tab runs rulesets against the loaded models, much like Solibri.

- A new project has no rulesets; *Add* offers the **built-in rulesets**, your
  own rulesets and **IDS 1.0** files (buildingSMART Information Delivery
  Specification). Each ruleset can be limited to some of the models.
  - *Clash: between disciplines*: every loaded model against the others.
  - *Clash: within each model*: an internal check of each model on its own.
    Fittings, terminals and dampers joined to the duct or pipe of their own
    system, and coverings such as insulation, are not reported.
  - Results are grouped by the discipline each model is set to in OpenCheck,
    not by the ruleset.
  - *Duplicates*: elements of one model with the same shape lying on top of
    each other, whatever their class or tessellation.
  - *Information: LOD DK*: building parts must carry the information their
    level of development requires (LOD 200, 300 or 325 DK). The level is
    set for the project on the ruleset card and per model in the model list.
    Where each item is read from (property, classification, material, …) can
    be adapted per project in *Where to look*, a window that can be made small
    or folded down to a button in the status bar while you look at the model.
    See [docs/information-requirements.md](docs/information-requirements.md).
  - *Clearance: MEP near structure*, *Electrical: cable tray clearances* and
    *Ventilation: clearances*.
- **Free zones**: room an element needs along its whole length, such as the
  service area beside a cable tray, the headroom above it or the space in
  front of a vertical ladder. The zone may be interrupted over a set length
  and can require a minimum that always stays free. The built-in *Electrical:
  cable tray clearances* ruleset uses them; the blocked part of the zone is
  drawn in the 3D view.
- **Script rules** for checks no rule type covers: a short script (Rhai) that
  measures runs, zones and distances and reports findings. Its numbers are
  input fields in the Ruleset Manager, and the code is meant to be written by
  a language model: *Copy prompt* hands it the description, the groups and
  the script reference, and *Use the answer* puts its code and parameters in
  place. See [docs/rule-scripts.md](docs/rule-scripts.md).
- The **Ruleset Manager** creates and edits rulesets and rules in forms (check
  type, severity, tolerance, groups A and B by discipline, IFC class and
  property), shows how many elements each group matches, explains why an
  element is in a group or not, keeps a personal library of rulesets, and
  runs checks.
- **Results** have a tab per ruleset and a section for each under *All*: an
  overview of discipline against discipline, then findings per pair, grouped
  by the kind of component. Information rules show the fill rate per building
  part and every element with what it is missing. Selecting a finding shows
  its two elements in orange and blue with the rest ghosted. Accept real
  problems and reject the rest; decisions are kept between runs.

Rulesets are TOML files; [docs/ruleset-authoring.md](docs/ruleset-authoring.md)
describes the format and can be handed to a language model to write custom
rulesets (the app offers it as a download).

### Issues and reports

- Tick findings (Ctrl+click, Shift+click or whole groups) and add them as
  **issues**. A dialog sets title, **criticality** C1 (not critical), C2
  (critical) or C3 (very critical) as Danish practice for consistency and
  clash checks recommends, status, **responsible disciplines** (by default
  those of the clashing models), the person it is assigned to (by initials;
  initials used once are offered again) and a description.
- Issues bundle several findings, can be merged, and keep their viewpoint:
  camera, section plane, what was hidden and whether the rest of the model
  was ghosted. Each object keeps its IFC GlobalId and its Revit Element ID.
- *Export* in the Issues tab writes the ticked issues (or all) as a **BCF 2.1
  file** (`.bcfzip`, with pictures and viewpoints) or as an illustrated
  **report**: one HTML file in Danish, printable to PDF, with introduction,
  models, software, an overview, and every issue with a small picture,
  criticality, responsible disciplines and the Revit IDs and GlobalIds of its
  objects.

### BCF Live (Dalux)

Issues can be shared with a BCF API 2.1 server; Dalux is the first. Sign in
through the browser, choose the project, pair OpenCheck's statuses,
criticality, type, phase and people with the server's values once, and send:
new issues become topics with their viewpoint and a picture. *Sync* brings
back status, assignee, title and priority changes and the comments. The
window shows the rights the server gives you in the project, and a
connection log helps when a server refuses something. See
[docs/bcf-live.md](docs/bcf-live.md).

### Visual Check

The *Visual Check* tab colours the model, so a pattern or a break in it
shows at a glance, and lists the problems; a click zooms to the element.
Each check is added on its own:

- **Alignment of walls and columns**: structural walls and columns standing
  in line from storey to storey share a colour; those out of line with the
  one above or below are red. Centre line or face alignment, with a
  tolerance.
- **Dimensions in pipe and duct systems**: each kind of system in its own
  muted colour; a pipe or duct larger than the piece before it, seen from
  the start of its system, is red.
- **Fall of drain pipes**: drain pipes green with enough fall, yellow with
  too little, red when flat or falling the wrong way; least fall overall
  and by pipe size.
- **Storeys**: each storey in its own colour; elements standing on another
  storey than they belong to are red, elements without a storey purple.

The system checks follow the ports of the elements (`IfcRelConnectsPorts`)
and, for loose elements, where their ends meet; sizes and falls are measured
from the geometry. Visual checks can run with autorun, their problems kept
as findings. See [docs/visual-checking.md](docs/visual-checking.md).

### Autorun and dashboard

*File → Autorun…* sets up checks that run without the app on a schedule:
the project's IFC files are read from their (network) paths, the chosen
rulesets and the ticked visual checks run, every run is kept as a compact,
compressed JSON file in a history, and a dashboard (one HTML file) shows the findings per
pair of disciplines, what is new and what was solved since the run before,
and the files' objects and sizes over time. The task runs `opencheck
autorun` from a bat file the app writes, and can be added to Windows Task
Scheduler from the dialog. A lock file keeps two machines from checking a
project at the same time. See [docs/autorun.md](docs/autorun.md).

### Look and feel

The "Grafit" design: graphite on paper, IBM Plex fonts, colour only where it
means something. *File → Theme* switches between the light design and a dark
one; the 3D view keeps its background in both.

The larger dialogs (Ruleset Manager, Where to look, BCF Live, Issue report,
Autorun) open within the screen, can be made smaller and scroll what does not
fit. On the desktop the button next to a dialog's close button opens it in a
window of its own, which can be moved outside OpenCheck's window, for
example onto a second screen; *Back into OpenCheck* puts it back. The choice
is remembered.

## Projects

A project is saved as one `.opencheck` file: a ZIP archive holding the IFC
files unchanged, the rulesets, IDS files, information requirements, checking
decisions, the working area, the visual checks, the autorun configuration,
issues and the view state. See
[docs/file-format.md](docs/file-format.md). Sign-ins to BCF servers are kept
per user, never in the project file.

## Command line

`opencheck.exe` inspects models and runs checks without the viewer:

```sh
opencheck info ARK.ifc KON.ifc VEN.ifc
opencheck tree ARK.ifc --elements
opencheck props ARK.ifc 1AQAupaRP1txwK1AGiN61V
opencheck rulesets
opencheck check ARK.ifc KON.ifc VEN.ifc --all
opencheck check ARK.ifc KON.ifc --ruleset information-lod-dk.toml --level 325
opencheck check ARK.ifc KON.ifc --ids delivery.ids
opencheck export-ids lod-300.ids --level 300
opencheck autorun \\server\share\project\OpenCheck\autorun.json
```

## Workspace layout

| Crate | Purpose |
|---|---|
| `opencheck-core` | IFC loading, element and property model, federation, rulesets and the check engine (clashes, clearances, duplicates, free zones, Rhai script rules), visual checks (alignment, pipe and duct systems with their port network, drain fall, storeys), information requirements and IDS, BCF data and files, project files, autorun runs and the dashboard |
| `opencheck-cli` | The `opencheck` command line tool, including `autorun` |
| `opencheck-app` | The application, for the desktop and the browser: federated 3D viewer (egui + wgpu), checking, visual checks, issues, reports, BCF Live and the autorun setup |

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

The browser version is built with [Trunk](https://trunkrs.dev):

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd crates/opencheck-app
trunk serve --release
```

and opened at <http://localhost:8080>. Every push to `main` publishes it on
GitHub Pages (`.github/workflows/pages.yml`).

To try it with the buildingSMART sample models:

```sh
sh scripts/fetch-testdata.sh
cargo run -p opencheck-app -- testdata/ifc4/Building-*.ifc
cargo run -p opencheck-cli -- check testdata/ifc4/Building-*.ifc --all
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Dependencies carry their own licenses; ifc-lite is MPL-2.0.
The IBM Plex fonts are under the SIL Open Font License
([OFL.txt](crates/opencheck-app/assets/fonts/OFL.txt)).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
