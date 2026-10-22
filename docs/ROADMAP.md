# Roadmap

What is planned for OpenCheck, roughly in order. The README describes what is
built.

## Done

**Model and viewer**

- Several IFC files (IFC2x3, IFC4) in one federated model; discipline guessed
  from the file name.
- 3D viewer: orbit around the cursor, zoom to element, storey filter,
  isolate, hide, X-ray, section plane with a draggable handle.
- Model tree and properties panel, with classifications, materials and
  systems.
- Laser and point-to-point measuring with snapping to corners and edges.
- Update a model to a newer revision; issue findings that disappear are
  marked "Solved by update".
- Working area ("Edit view"): a rectangle drawn in a top view, aligned to an
  element if wanted; only elements touching it are shown and checked.
- Walk mode: stand on a floor and walk through the model at eye height.
- Right-click in the model tree to isolate, hide, show, select or zoom to a
  model, storey, class or element.
- `.opencheck` project file with File menu (see [file-format.md](file-format.md)).

**Checking**

- TOML rulesets with clash (with tolerance), clearance, duplicate and
  information rules; built-in rulesets; model scope per ruleset; Ruleset
  Manager with form editor, element pickers, "Try the rule" and a personal
  library; downloadable ruleset guide for language models.
- Standard rulesets in every project: duplicates compared by geometry, and
  information requirements per building part and LOD level (200, 300,
  325 DK) with the level per project and per model.
- Information requirements adapted per project: where each item is read
  from, for all building parts or one; "found in" per item in the results.
- IDS 1.0: import `.ids` files and check them (app and `check --ids`);
  export the built-in standard as IDS (`export-ids` and the Add menu). The
  official buildingSMART test cases run against the implementation.
- Free zone rules (service areas, headroom, room in front, along the whole
  element, with allowed interruptions and a hard minimum) and script rules
  (Rhai) written with a language model; built-in cable tray clearances.
- Results with a tab and a section per ruleset, discipline matrix, grouping
  by component, multi-select, accept / reject kept between runs.

**Issues and sharing**

- Issues bundling several findings, merging, saved viewpoints, C402
  criticality (C1–C3), responsible disciplines, assignee by initials, Revit
  Element IDs, a dialog for the fields when issues are created.
- BCF 2.1 file export with pictures and viewpoints.
- Illustrated issue report (HTML in Danish, printable to PDF).
- BCF Live with Dalux: browser sign-in, value mapping (statuses,
  criticality, type, phase, people), rights shown per project, topics sent
  with viewpoint and picture, sync of status, assignee, title and priority,
  comments, connection log.

**Web version**

- The same code compiled to WebAssembly and published on GitHub Pages for
  every push to `main`. IFC files are opened in the browser and stay there;
  projects, issues, reports and BCF files are downloaded, and projects are
  opened again from the file. File dialogs, downloads and
  background work go through one small layer (`platform.rs`).

**Look and feel**

- "Grafit" design with IBM Plex fonts; light and dark theme.

## In progress

- **Releases**: a GitHub Actions workflow that builds Windows downloads for
  every version tag and publishes them on the Releases page.
- Testing the whole workflow on real projects: Revit models → IFC →
  OpenCheck → BCF in Dalux and Revit, including whether cameras land in the
  same place.

## Next

- **BCF, next steps**: import BCF files and open topics from the server in
  the viewer (camera, section, selection); a coordinate offset per project if
  Revit or other tools place cameras elsewhere; more servers than Dalux;
  automatic sync.
- **Reports, next steps**: report templates (layout and fields), the IKT
  manager's summary report, PDF without going through the browser.
- **Working area, next steps**: limits in height (storeys), several areas,
  named areas to switch between.
- **Information requirements, next steps**: allowed values and value
  patterns; choosing which items each part requires per level in the
  project.
- **IDS, next steps**: `IFCRELNESTS`, voids and fills relations; predefined
  types and classification hierarchies from type objects; storeys and other
  spatial nodes as applicable elements.
- **Free zones and scripts, next steps**: follow a run across its segments
  and fittings, so interruptions that continue into the next segment add up;
  zones in front of panels and switchboards (which face is the front); stair
  and door clearances; tray-to-tray spacing by system type (power, IT).
- More rule types: space checks, model QA (missing storeys, elements without
  containment).
- Model update report: what changed between two revisions (added, removed
  and changed elements).
- Performance on large models: a triangle BVH per element for clash checks.

## Later

- **Code signing** of the Windows build, so SmartScreen does not warn; builds
  for macOS and Linux.
- **Web version, next steps**: recent projects kept in browser storage;
  updating a model in place; the theme and ruleset library remembered; loading and
  checking in a Web Worker so the page does not wait; BCF Live (needs a
  sign-in that returns to the page and servers that allow it); WebGL2 for
  browsers without WebGPU. Known limits: about 4 GB memory per tab, slower
  geometry processing.
