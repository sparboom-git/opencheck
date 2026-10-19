# Roadmap

What is planned for OpenCheck, roughly in order. Items move to the README
once they are built.

## Done

- Load several IFC files (IFC2x3, IFC4) into one federated model.
- 3D viewer: orbit around the cursor, zoom to element, storey filter,
  isolate, hide, X-ray, section plane with a draggable handle.
- Model tree and properties panel.
- Laser and point-to-point measuring with snapping to corners and edges.
- `.opencheck` project file with File menu (see [file-format.md](file-format.md)).
- Checking: TOML rulesets, six built-in rulesets, model scope per ruleset,
  clash (with tolerance), clearance and duplicate rules, results with accept /
  reject kept between runs, CLI `check` command, downloadable ruleset guide
  for language models.
- Ruleset Manager with form editor and personal ruleset library; results
  grouped by category and component; multi-select; selected issues (BCF-style)
  saved in the project.

## In progress

- Polishing checking on real projects: performance on large models
  (triangle BVH per element), more built-in rulesets.

## Next

- **Issues, next steps**: comments, assignee, snapshots of the viewpoint,
  merging several findings into one issue.
- **Reports**: a report is a selection of issues that are relevant for one
  check run and must be handled. Report templates define layout and fields;
  output as PDF and HTML.
- **BCF export / import** of issues.
- **BCF Live connector**: synchronise issues with an issue platform through the
  BCF API, starting with Dalux, so issues can be commented on there.
- **Custom rulesets with an LLM**: the application offers a downloadable
  Markdown guide that describes the ruleset format. A user gives it to their
  preferred language model, describes the rules they need, and imports the
  resulting TOML file. The guide lives in
  [ruleset-authoring.md](ruleset-authoring.md).
- More rule types: property checks (required properties and allowed values),
  classification checks, space checks, model QA (missing storeys, elements
  without containment).
- Update model: replace an IFC file with a newer revision and compare.

## Later

- **Web version**: the same Rust code compiled to WebAssembly and served as a
  static website. IFC files are opened and stay in the browser (nothing is
  uploaded to a server); projects are kept in browser storage and can be
  downloaded as `.opencheck` files. To keep this possible:
  - `opencheck-core` must not depend on the file system or threads in its
    API; it works on bytes.
  - File access and background work in the app go through one small layer
    that can be swapped for browser APIs (File System Access, Web Workers).
  - A CI job should compile `opencheck-core` for `wasm32-unknown-unknown`.
  Known limits: about 4 GB memory per tab, WebGPU only in Chromium browsers
  today (WebGL2 needs a shader fallback), slower geometry processing.
