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

> Status: under development and not yet ready for production use. Viewing,
> checking (including free zones and script rules), visual checks,
> information requirements, IDS, issues, reports, BCF files, BCF Live with
> Dalux and autorun work, on the desktop and largely in the browser, but
> expect bugs, unfinished parts and changes between versions. See
> [docs/ROADMAP.md](docs/ROADMAP.md) for what comes next.
>
> OpenCheck comes without warranty, and the use of it and of its results is
> the user's own responsibility: see the [Disclaimer](#disclaimer).

## Download

You do not need to build OpenCheck to try it. For Windows:

1. Open the [Releases page](https://github.com/sparboom-git/opencheck/releases)
   and download `OpenCheck-<version>-windows-x64.zip` from the latest release.
2. Unzip it anywhere, for example on your desktop. Nothing is installed.
3. Double-click `opencheck-app.exe` to start the viewer. `opencheck.exe` is the
   command line tool.

The program is not code-signed, so Windows SmartScreen may show "Windows
protected your PC" the first time. Click **More info** and then **Run anyway**.

A Microsoft Store version is being prepared; see
[docs/microsoft-store.md](docs/microsoft-store.md) for how it is packaged and
how its version numbers work. The `.msix` file of a release is the package
for the Store: it is not signed, so download the ZIP to run OpenCheck.

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
- **WIP, features under development**: switched on in *File → Settings*, and
  off until you do. With it off OpenCheck offers only what is stable. In WIP
  now: the clearance rulesets (MEP near structure, cable trays, ventilation), the
  check of classification codes against BIMTypeCode 2022 DK,
  the visual checks of dimensions in pipe and duct systems and of the fall of
  drain pipes, adding Revit files (`.rvt`), converted to IFC by
  [rvt-rs](https://github.com/DrunkOnJava/rvt-rs) 0.25.0 (see
  [docs/revit-files.md](docs/revit-files.md)), and adding **coordination
  files** (`.smc`), the file a model checker saves with the IFC
  models of a project put together: each model in it becomes a model here,
  with its elements, properties, storeys, colours and shapes, stored in the
  project as IFC, read by the reader of
  [Open BIM Suite](https://github.com/OpenAEC-Foundation/open-bim-suite)
  (LGPL-3.0-or-later; see [docs/smc-files.md](docs/smc-files.md)). The
  other way, *File → Export → Coordination file (.smc)…* writes the models
  of a project as one coordination file, with their elements, properties,
  storeys, types, materials, systems, classification codes and shapes as
  triangles. All
  desktop only. A ruleset or check already in a project stays when WIP is
  switched off.
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
| K, or the markers button (crosshair) | Clash markers: a circle at each clash in sight nearby (within 15 m of the eye when walking, of the point looked at otherwise), as filtered in the Check tab's list. The colour is the category (C3 red, C2 orange, C1 grey), a ring with a tick or a flag means decided or in an issue. Hover for the clash, click to open it in a window with the same actions as in the Check tab; a click elsewhere lets go of it. Right-click the button to choose which clashes (the Check tab's filters: status, severity, rule) and the distance |
| Right-click in the 3D view | Also the clash markers and their distance, the finding window, on the Check and Visual Check tabs the properties window, and a section parallel to the clicked face (e.g. vertical along a wall); drag the scissors to move it |

### Checking

The Check tab runs rulesets against the loaded models, as in other model
checkers.

- A new project has no rulesets; *Add* offers the **built-in rulesets**, your
  own rulesets and **IDS 1.0** files (buildingSMART Information Delivery
  Specification). Each ruleset can be limited to some of the models. The
  pin next to *Add* lets the rulesets panel hide itself, leaving the room to
  the 3D view and the results: it comes out over the view with the mouse on
  the bar at the left edge. The choice is remembered. The Visual Check tab's
  panel of checks has the same pin, with a choice of its own.
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
    set for the project under *File → Project information level* (or on the
    ruleset card), and per model in its menu in the model list; models without
    their own level follow the project.
    Where each item is read from (property, classification, material, …) can
    be adapted per project in *Where to look*, a window that can be made small
    or folded down to a button in the status bar while you look at the model.
    See [docs/information-requirements.md](docs/information-requirements.md).
    With *WIP* on, the tick *Classification* on its card also checks the
    classification codes themselves against a classification system in a
    named edition, for now **BIMTypeCode 2022 DK**: the code is one of the
    list's codes (not a group, not a code marked *FRI*), the type id begins
    with it, and it fits the element, e.g. no pipe code on a wall. The
    classification code and the type id are then read only where that system
    keeps them, so a CCS or CCI code alone no longer counts. The results get
    a section of their own, between the fill rates and the missing
    information.
  - *Clearance: MEP near structure*, *Electrical: cable tray clearances* and
    *Ventilation: clearances*. These three are **work in progress**: they are
    offered only with WIP switched on in *File → Settings*, apart, under
    *Work in progress* in the Add menu and *WIP* in the Ruleset Manager, and
    marked WIP when added.
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
- **Leave out…** on a ruleset card (and under each rule in the Ruleset
  Manager) is a table of what the check leaves out: per row the discipline's
  models, then a category (walls, columns, coverings, ducts, …), then if
  needed a property with *contains*, *is*, *is not*, *starts with*, *is
  under*, *is over* (numbers, e.g. pipes under 50 mm) or *exists*. Each row shows how many elements it leaves out and can be
  switched off. It works for clashes between disciplines and within each
  model alike.
- The **Ruleset Manager** creates and edits rulesets and rules in forms (check
  type, severity, tolerance, groups A and B by discipline, IFC class and
  property), shows how many elements each group matches, explains why an
  element is in a group or not, keeps a personal library of rulesets, and
  runs checks.
- **Results** have a tab per ruleset and a section for each under *All*: an
  overview of discipline against discipline, then findings per pair, grouped
  by the kind of component. Information rules show the fill rate per IFC file
  and building part, and every element with what it is missing; with the
  classification check on also the share of codes in order per IFC file and
  the elements with a wrong code. Selecting a finding shows
  its two elements in their models' colours with the rest ghosted. Accept real
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
  file** (`.bcfzip`, with pictures and viewpoints) or as an **Excel workbook** (`.xlsx`) on A4 portrait:
  one row per issue with its fields, a picture and its objects' Revit IDs and GlobalIds,
  and a sheet with the model files the check is based on and their dates.
  The workbook is made from a template: OpenCheck's standard one, or your
  own with your company's name and layout (*File → Settings*); see
  [docs/excel-templates.md](docs/excel-templates.md).

### Documented checks

A visual check can be written down as done, to show that the models were
checked and how. *Document check…* beside a visual check's result opens a
record of it: a title, what the check does (written by OpenCheck from its
settings), what was checked and how, who checked it and the
result as a traffic light: **Approved** (green), **Approved with remarks**
(orange) or **Not approved** (red). It keeps the colours with how many
elements had each, the models and their versions, and pictures of the view;
turn the view and add more pictures for large models. In reports the
colours are one short line, at most 16 of them, or left out when the
pictures say enough. No elements are kept. The record is kept as you
write: there is nothing to save in the dialog, and closing it loses nothing;
*Delete…* takes it away again.
Problems found are made issues as usual. The documented checks are listed on
the Visual Check tab and saved with the project.

### Reports

The **Report** tab builds the written documentation of the check, after the
Danish C402 practice: a report in Danish with introduction (prepared by,
checked by, company, disciplines, phase), basis (models with their versions
and companies, software and rulesets), the check (general remarks,
properties, geometry, clashes) and a conclusion. A new report starts with
those chapters; blocks can be added, moved and deleted, and each has a
heading, a text and, for issues, filters (criticality, status, responsible
discipline, ruleset), issues left out by hand and a comment per issue.

- **Revisions and phases**: a report keeps its revisions. A new report is
  made for a phase (Disp, PF, MP, UBP, UFP or SU: dispositionsforslag to som
  udført) and starts without a revision, as Danish practice has it; *Issue
  revision* locks it as revision A (or the label typed) with date, who
  issued it and what changed. Each revision freezes its data when made: the
  issues with their pictures, the models and the rulesets. *Update data →
  new version* takes the latest check into the next revision (B, C, …) of
  the phase, or into a new phase, which starts without a revision again.
  The date printed in the report is typed by hand in the introduction block;
  empty, it is the date of issue. Every revision stays in the project and
  can be opened and exported again.
- **Text and pictures**: *Add block → Tekst (frit afsnit)* adds a blank
  section for free text. Screenshots can be pasted into any block (*Paste
  picture*, or Ctrl+V while editing) or added as PNG files; they are shown
  after the text, as wide as the text column, a small crop at its own size
  and a tall one kept to half a page, in the PDF, in Word and in the
  preview.
- **Preview** beside the editor shows the report set in A4 pages, exactly as
  it prints, or as a quick draft without pages. A block chosen on the left is
  scrolled into view; a block clicked in the preview is edited.
- **Export PDF** and **Export Word** write the revision shown, with a table
  of the revisions (phase, revision, date, issued by, change) on the first
  page and page header and footer. OpenCheck sets the PDF itself (with
  [Typst](https://typst.app)); nothing else is needed. *Foreløbigt tryk*
  stamps a watermark across every page of the preview and both exports, for
  a trial print that is clearly not the valid document.
- **Visual checks**: the section *Visuelle kontroller* shows the documented
  checks with their result, method, colours and pictures; checks can be left
  out by hand.
- **Information level**: the section *Informationsniveau* shows each
  information check (LOD) of the last run: how it was done, written by
  OpenCheck in Danish, and the fill rate per IFC file and building part with
  the items under each. Accepted items are marked as OK in blue. With the
  classification check on, a card after the fill rates names the system,
  its edition and licence, and shows per model the share of codes in order
  and the most frequent problems.
- **Template**: reports are made from a Word template. *File → Settings*
  saves the standard one to change in Word (logo, header and footer, page
  margins, fonts, colours, the Heading 1 and Heading 2 styles, the text on
  the first page) and choose again. Fields such as `{{project}}`,
  `{{revision}}` and `{{company}}` mark where the report's values go;
  `{{revisions}}` and `{{content}}`, each alone in a paragraph, where the
  revision table and the chapters go. A Word export is the template filled
  in; the PDF and the preview follow the same template, so the two read
  alike. The preview follows the template file as it is saved.

### BCF Live (Dalux)

Issues can be shared with a BCF API 2.1 server; Dalux is the first. Sign in
through the browser, choose the project, pair OpenCheck's statuses,
criticality, type, phase and people with the server's values once, and send:
new issues become topics with their viewpoint and a picture. *Sync* brings
back status, assignee, title and priority changes and the comments. The
window shows the rights the server gives you in the project, and a
connection log helps when a server refuses something. See
[docs/bcf-live.md](docs/bcf-live.md).

### Clash triage with Claude

A check often gives thousands of findings of which only a few matter.
*File → Claude connection…* lets **Claude Desktop** review the findings of
the open project through an MCP server (`opencheck mcp`): it works through
groups of alike findings and suggests a C402 category for each (C1 not
critical, which includes all noise; C2 critical; C3 very critical), with how
sure it is and a short reason. The suggestions show up in the Check tab's
*Pre-sort* tab as they come in, apart from the results so they do not get in
the way of your own sorting; the results can be filtered by the category decided (C1, C2, C3);
approved C1 findings are marked not a problem, C2 and C3 become
issues with that priority. Corrections are kept as examples in the project's
triage file, so the next review follows the project's practice. Pre-sorting,
switched on per clash rule, marks known noise (insulation around its own
pipe, the same element twice, shallow overlaps, planned openings, and kinds
of clashes you marked as noise from one of them) before anyone looks. Claude only suggests; it cannot approve or change anything. See
[docs/claude-triage.md](docs/claude-triage.md).

### Visual Check

The *Visual Check* tab colours the model, so a pattern or a break in it
shows at a glance, and lists the problems; a click zooms to the element.
Some checks also draw marks on the elements they point out, arrows and white
rings, which the crosshair button in the 3D view (or `K`) switches off and
on. Each check is added on its own:

- **Alignment of walls and columns**: structural walls and columns standing
  in line from storey to storey share a colour; those out of line with the
  one above or below are red. Centre line or face alignment, with a
  tolerance. Everything else is hidden, so only the walls and columns show;
  *The other elements* brings it back, ghosted or as it is.
- **Dimensions in pipe and duct systems** (WIP, offered with WIP switched on
  in *File → Settings*): each kind of system in its own
  muted colour; a pipe or duct larger than the piece before it, seen from
  the start of its system, is red, with a white ring on it so it is found
  in the view.
- **Fall of drain pipes** (WIP, as above): drain pipes green with enough
  fall, yellow with
  too little, red when flat or falling the wrong way; least fall overall
  and by pipe size. An arrow on each pipe shows the way it falls and a
  ring marks a flat pipe: white on the red pipes and faint on the rest,
  sized with the model so they show when zoomed in; in 3D, in plan and in
  the pictures of a documented check.
- **Storeys**: each storey in its own colour; elements standing on another
  storey than they belong to are red, elements without a storey purple,
  both with a white ring on them.
- **Rooms**: rooms (`IfcSpace`) green. Per storey the walls, doors, windows
  and columns of the chosen models (usually architect and structure) are cut
  above the floor; a place they enclose without a room is an area without a
  room, its walls red. A button per storey shows it in plan: cut above its
  floor, seen from above.
- **Levels**: the storeys of all models side by side by height, as a table
  and as levels drawn in the 3D view, a colour per model. A level with
  different names, or a name at different heights, is red.

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
autorun` (the app also writes a bat file for running it by hand) and can be
added to Windows Task Scheduler from the dialog. A lock file keeps two machines from checking a
project at the same time. See [docs/autorun.md](docs/autorun.md).

### Look and feel

The "Grafit" design: graphite on paper, IBM Plex fonts, colour only where it
means something. *File → Theme* switches between the light design and a dark
one; the 3D view keeps its background in both.

The larger dialogs (Ruleset Manager, Where to look, BCF Live, Settings,
Autorun) open within the screen, can be made smaller and scroll what does not
fit. On the desktop the button next to a dialog's close button opens it in a
window of its own, which can be moved outside OpenCheck's window, for
example onto a second screen; *Back into OpenCheck* puts it back. The choice
is remembered.

### Logs and crash reports

The desktop app keeps a log of each of its last ten sessions in
`%APPDATA%\OpenCheck\logs`: how it started (version, system, graphics
card), the warnings and errors, and how the session ended. When the program
fails it writes a crash report there: what failed, where, and the last
lines of the log. A session that did not close normally is told at the next
start, with a button showing the file. *File → Settings → Open the log
folder* opens the folder. Nothing is sent from OpenCheck: the user sends
the files when reporting a problem. They hold no model content, but file
names and folder paths can appear in them. When the system itself ends the
program (out of memory, a fault in the graphics driver) no report can be
written; the log still shows that the session did not close.

## Projects

A project is saved as one `.opencheck` file: a ZIP archive holding the IFC
files unchanged (for a Revit or coordination file, the IFC it was converted
to), the rulesets, IDS files, information requirements, the
classification system chosen, checking decisions, suggested categories and the triage file, the working area, the
visual checks, the autorun configuration, issues and the view state. See
[docs/file-format.md](docs/file-format.md). Sign-ins to BCF servers are kept
per user, never in the project file.

## Command line

`opencheck.exe` inspects models and runs checks without the viewer:

```sh
opencheck info ARK.ifc KON.ifc VEN.ifc
opencheck tree ARK.ifc --elements
opencheck props ARK.ifc 1AQAupaRP1txwK1AGiN61V
opencheck rulesets
opencheck classifications
opencheck check ARK.ifc KON.ifc VEN.ifc --all
opencheck check ARK.ifc KON.ifc --ruleset information-lod-dk.toml --level 325
opencheck check ARK.ifc VVS.ifc --ruleset information-lod-dk.toml --classification bimtypecode-2022-dk
opencheck check ARK.ifc KON.ifc --ids delivery.ids
opencheck export-ids lod-300.ids --level 300
opencheck convert project.smc project.opencheck
opencheck export-smc A.ifc K.ifc --output project.smc   # IFC models as one coordination file (WIP)
opencheck mcp
opencheck autorun \\server\share\project\OpenCheck\autorun.json
```

## Workspace layout

| Crate | Purpose |
|---|---|
| `opencheck-core` | IFC loading, element and property model, federation, rulesets and the check engine (clashes, clearances, duplicates, free zones, Rhai script rules), visual checks (alignment, pipe and duct systems with their port network, drain fall, storeys, rooms, levels), triage of findings (C402 categories, groups, pre-sorting), information requirements, classification systems and IDS, BCF data and files, project files, autorun runs and the dashboard |
| `opencheck-rvt` | Revit files (WIP): converts `.rvt` to IFC4 with rvt-rs; the only code that knows rvt-rs |
| `opencheck-smc` | Coordination files (`.smc`, WIP): the reader of Open BIM Suite and the writing of each model as IFC4, and a writer of coordination files from models; the only code that knows the format. LGPL-3.0-or-later, unlike the rest |
| `opencheck-cli` | The `opencheck` command line tool, including `autorun` and the `mcp` server for Claude Desktop |
| `opencheck-app` | The application, for the desktop and the browser: federated 3D viewer (egui + wgpu), checking, visual checks, issues, reports, BCF Live, the autorun setup and the Claude connection |

IFC parsing and tessellation are provided by
[ifc-lite](https://github.com/LTplus-AG/ifc-lite) (MPL-2.0). OpenCheck wraps it
in its own types in `opencheck-core`, so the rest of the code does not depend on
the ifc-lite API. The wiki's
[Architecture](https://github.com/sparboom-git/opencheck/wiki/Architecture)
page describes how the crates work together, what is stored where, and what
leaves your computer (nothing, unless you switch a connection on).

Revit files are read by [rvt-rs](https://github.com/DrunkOnJava/rvt-rs) by
Griffin Long (Apache-2.0), pinned at 0.25.0, which converts them to IFC4
without Revit. This is a feature under development, switched on with *File →
Settings → WIP*;
[docs/revit-files.md](docs/revit-files.md) says what it does, what to be
careful about and how a new rvt-rs release is taken in.

Coordination files (`.smc`) are read by the reader of
[Open BIM Suite](https://github.com/OpenAEC-Foundation/open-bim-suite) by the
OpenAEC Foundation (LGPL-3.0-or-later), copied into `opencheck-smc`, which
writes each model as IFC4. This too is a feature under development, behind
the same WIP switch. [docs/smc-files.md](docs/smc-files.md) says what
is converted, what is not, what the licence means and how a new revision of
the reader is taken in.

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

## Disclaimer

OpenCheck is under development and is not yet ready for production use. It
is an aid to model checking. It does not replace the checking, the quality
assurance and the professional judgement a project requires.

- **No check is complete.** OpenCheck can miss clashes, missing information
  and other errors in a model, and it can report things that are not errors.
  A check without findings does not show that the models are free of errors.
- **The results depend on what goes in.** What is found depends on the IFC
  files as they were exported, on how OpenCheck reads their geometry and
  properties, and on the rulesets, tolerances and exceptions chosen.
- **The use is the user's responsibility.** Whoever uses OpenCheck is
  responsible for how it is used, for verifying its results and for every
  decision made from them. That holds for the reports, issues and BCF files
  it writes as well.
- **No warranty, no liability.** OpenCheck is provided "as is", without
  warranty of any kind. To the extent the law allows, the authors and
  contributors are not liable for any loss or damage arising from its use,
  including errors a check did not find. The terms that apply are those of
  the licences below: the MIT licence, and sections 7 and 8 of the Apache
  License 2.0.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option, except the crate `opencheck-smc`, which is under the GNU
Lesser General Public License, version 3 or later: it is built on the reader
of coordination files of Open BIM Suite by the OpenAEC Foundation, and its
licence and notice are in
[crates/opencheck-smc/licenses](crates/opencheck-smc/licenses). Dependencies
carry their own licenses; ifc-lite is MPL-2.0.
rvt-rs, copyright 2026 Griffin Long, is under the Apache License 2.0; its
licence and notices are in
[crates/opencheck-rvt/licenses](crates/opencheck-rvt/licenses). rvt-rs and
OpenCheck are not affiliated with Autodesk; "Autodesk" and "Revit" are
trademarks of Autodesk, Inc. Open BIM Suite and OpenCheck are not affiliated
with the maker of any model checker.
The IBM Plex fonts are under the SIL Open Font License
([OFL.txt](crates/opencheck-app/assets/fonts/OFL.txt)).
The list of codes of BIMTypeCode 2022 DK, by [BIM7AA](http://BIM7AA.dk), is
included unchanged under
[CC BY-ND 4.0](https://creativecommons.org/licenses/by-nd/4.0/); see
[BIMTypeCode-NOTICE.txt](crates/opencheck-core/classifications/BIMTypeCode-NOTICE.txt).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
