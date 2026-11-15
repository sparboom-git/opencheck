# Revit files (WIP)

OpenCheck can add a Revit file (`.rvt`) as a model. This is a feature under
development: it is offered only when **WIP** is switched on in *File →
Settings*, and only in the desktop app.

## Credit

Reading Revit files is the work of
[rvt-rs](https://github.com/DrunkOnJava/rvt-rs) by Griffin Long, an
independent open source project under the Apache License 2.0. It reads Revit
files without Revit, from public observations of the file format, and writes
them as IFC4. OpenCheck calls it as a library and adds nothing to the
conversion itself. Problems with what a Revit file converts to are best
reported to rvt-rs, with the diagnostics its own `rvt-ifc` tool writes.

rvt-rs is not affiliated with, endorsed by or sponsored by Autodesk. "Autodesk"
and "Revit" are trademarks of Autodesk, Inc.

The licence and notices of rvt-rs are in
[`crates/opencheck-rvt/licenses`](../crates/opencheck-rvt/licenses), are shown
in *File → Settings → WIP*, and are shipped with every release of OpenCheck.

## How it works

1. A `.rvt` file is added like an IFC file: *File → Add IFC or Revit models*,
   or dropped on the window. A model can also be updated from a `.rvt` file.
2. rvt-rs converts it to IFC4 in memory. This takes a while: about a minute
   for a 160 MB file.
3. OpenCheck reads that IFC as it reads any other. From here on nothing in
   OpenCheck knows the model came from Revit.
4. A window says what was converted: how many elements, how many of them
   only as a box, how rvt-rs rates its support for that Revit release, and
   the notes rvt-rs itself gives.
5. When the project is saved, the converted IFC is stored in it. The Revit
   file is not needed to open the project again.

## What to be careful about

rvt-rs says itself that it is a reader, not a converter for every model, and
how much it reads depends on the Revit release. What matters for checking:

- **Boxes.** Many elements come with the right place and size but as a box,
  not their real shape. Clash and clearance checks against such elements find
  things that are not there and miss things that are.
- **Classes.** Revit's *Generic Model* becomes `IfcBuildingElementProxy`, so
  rules written for a class do not find those elements.
- **Parameters.** Some parameters are not read, so information checks can
  report something as missing that is in the Revit model.
- **Revit releases.** rvt-rs rates each release as verified, partial or
  experimental; the window after a conversion says which.

An IFC exported from Revit itself is still the model to rely on.

## Where the code is

| Where | What |
|---|---|
| `crates/opencheck-rvt` | The only code that knows rvt-rs: Revit file in, IFC bytes and a summary out |
| `crates/opencheck-app/src/wip.rs` | The WIP switch, its part of the Settings dialog and the conversion window |
| `crates/opencheck-app/src/jobs.rs` | `load_revit_file`: converts in the background and loads the IFC |

The browser version and the `opencheck` command line tool do not read Revit
files.

## Taking in a new rvt-rs release

rvt-rs releases often, so its version is pinned. To move to a newer one:

1. In the workspace `Cargo.toml`, change `rvt = "=0.20.0"` to the new version.
2. In `crates/opencheck-rvt/src/lib.rs`, change `CONVERTER_VERSION` to match.
3. Run `cargo update -p rvt`.
4. Copy `LICENSE` and `NOTICE` from the new release over
   `crates/opencheck-rvt/licenses/rvt-rs-LICENSE` and `rvt-rs-NOTICE`, in case
   they changed.
5. Run `cargo test -p opencheck-rvt`. A test fails if the version in step 2
   is not the one that is built in.
6. Convert a Revit file and compare with the previous result, in the app or
   without it:

   ```sh
   cargo run -p opencheck-rvt --example rvt2ifc -- model.rvt out.ifc
   ```

If rvt-rs changed the functions OpenCheck calls, the build fails in
`crates/opencheck-rvt/src/lib.rs` and nowhere else: that file is all there is
to adapt. It uses `RevitFile::open`, `ifc::run_export`, `ifc::ExportOptions`
and `ifc::step_writer::write_step_to`, which rvt-rs lists among the modules it
keeps stable from its 1.0.
