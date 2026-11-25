# Coordination files (.smc, WIP)

OpenCheck can add a coordination file (`.smc`), the file a model checker
(Solibri) saves with the IFC models of a project put together. Each model in
it becomes a model in OpenCheck, with the elements, properties, storeys,
colours and shapes the checker kept. This is a feature under development: in
the app it is offered only when **WIP** is switched on in *File → Settings*,
and only in the desktop app; the command line has it always. It has been
tried on few files so far, and another checker version or another project may
hold objects the reader does not know.

## Credit

Reading these files is the work of
[Open BIM Suite](https://github.com/OpenAEC-Foundation/open-bim-suite) by the
OpenAEC Foundation, an open source 3D viewer for building models under the
GNU Lesser General Public License, version 3 or later. Its authors worked out
how a coordination file is built and wrote the reader of it; OpenCheck copies
that reader (two files, `stream.rs` and `mesh.rs`, from revision
`eb4142ee12ea28f796c07f475a24a1d0c66aa8f0` of 2026-11-25) into the crate
`opencheck-smc` and adds the writing of each model as IFC. Without that work
OpenCheck would not read these files.

Open BIM Suite and OpenCheck are not affiliated with Solibri. Solibri is a
trademark of Solibri Oy (a Nemetschek company); the name says which program
writes the files.

The licence and notice of the reader are in
[`crates/opencheck-smc/licenses`](../crates/opencheck-smc/licenses), are shown
in *File → Settings → WIP*, and are shipped with every release of OpenCheck.

## Licence

`opencheck-smc` is under the **LGPL-3.0-or-later**, the licence of the reader
it is built on. The rest of OpenCheck stays under MIT or Apache-2.0, and that
is why the reader is a crate of its own: the LGPL stays with the code it
covers, and nothing else in OpenCheck depends on how that code works.

The desktop app and the command line tool link `opencheck-smc` statically.
The LGPL allows that when the user can modify the library and relink the
application with the modified version (LGPL-3.0, section 4(d)(0)). OpenCheck's
complete source code is public under permissive licences, so anyone can
rebuild it with a changed `opencheck-smc`. Each release ships
`open-bim-suite-LICENSE.txt` and `open-bim-suite-NOTICE.txt` next to the
executables.

What this means in practice:

- Changes to `crates/opencheck-smc` are under the LGPL, including OpenCheck's
  own files there (`lib.rs`, `ifc.rs`).
- Code must not be copied from `opencheck-smc` into the other crates, which
  are MIT or Apache-2.0.
- The browser version does not include the crate, so nothing changes for it.

## How it works

1. With WIP on, a `.smc` file is added like an IFC file: *File → Add IFC,
   SMC or Revit models*, dropped on the window, or given on the command line.
   It cannot be used to *update* a model: it holds a whole project.
2. `opencheck-smc` reads the file. It is a zip around one stream of serialized
   Java objects, the checker's own classes; there is no IFC inside. The reader
   takes the models, their elements with GlobalId, class, name, type, tag and
   property sets, the spatial elements with their names and elevations, and
   the shapes as triangles with their colours.
3. Each model is written as an IFC4 file in memory: the elements as their IFC
   class with `IfcTriangulatedFaceSet` bodies in metres, `IfcPropertySet`s,
   `IfcStyledItem` colours, and the site, building and storeys with
   `IfcRelAggregates` and `IfcRelContainedInSpatialStructure`. An element is
   put on the storey its chain of coordinate frames leads to.
4. OpenCheck loads those IFC files in the file's order, so the first model
   gives the federation its origin, as with IFC files. From here on nothing
   in OpenCheck knows the models came from a coordination file.
5. A window says what was converted: the models with their element counts
   and the notes of the conversion.
6. When the project is saved, the converted IFC files are stored in it; the
   `.smc` is not needed to open the project again.

The models are named after the IFC files the checker read (`TDH_K09_F2_N01`),
when the file records them, else after the checker's short name (`K`). The
checker's short name, set by the coordinator, decides the discipline where it
says one (`A` architectural, `K` structural, `L` landscape, `E` electrical,
`R` plumbing, `VENT` ventilation); otherwise it is guessed from the file name
as for any IFC.

Georeferenced models keep their precision: every element is written relative
to one project origin, the first corner found rounded down to 100 m, and
placed by one `IfcLocalPlacement` at that origin.

## What to be careful about

The models are what the checker kept, not the original IFC files:

- **Shapes** are the checker's triangulation of the IFC geometry: fine for
  clashes and clearances, but not the IFC representation, and openings are
  cut from their hosts already and not written themselves.
- **Colours**: one per element, the colour most of its corners have. A window
  with a frame and glass gets the frame's colour.
- **Not written**: type objects and `IfcRelDefinesByType`, quantities as
  `IfcElementQuantity` (they come as properties), systems and their members,
  ports and connections, materials, and the checker's own issues and
  presentations.
- **Classes** that IFC4 has no concrete entity for are written as
  `IfcBuildingElementProxy`; the window names them.
- **Elements without a storey** in the file go under the model's first storey;
  the window says how many.

An IFC exported from the authoring tool is still the model to rely on: update
a model from its own IFC file with the ↻ button on its card when you have it.
Name, discipline, colour and the user's picks carry over, since the GlobalIds
are the same.

## Command line

```sh
opencheck convert project.smc                      # writes project.opencheck next to it
opencheck convert project.smc C:\Checks\project.opencheck
```

The project has one IFC per model, with its discipline and hash, and no
rulesets yet; open it in the app or give its models to `opencheck check`.

## Where the code is

| Where | What |
|---|---|
| `crates/opencheck-smc/src/stream.rs`, `mesh.rs` | The reader, from Open BIM Suite (each change marked `OpenCheck:`) |
| `crates/opencheck-smc/src/ifc.rs` | The models as IFC4 files |
| `crates/opencheck-smc/src/lib.rs` | `convert_file`, the summary, the credit constants |
| `crates/opencheck-smc/licenses` | The licence and notice of the reader |
| `crates/opencheck-app/src/smc.rs` | The credit in the WIP part of Settings |
| `crates/opencheck-app/src/wip.rs` | The WIP switch, the feature's entry and the conversion window |
| `crates/opencheck-app/src/jobs.rs` | `load_smc_file`: converts in the background and loads the models |
| `crates/opencheck-cli/src/main.rs` | `opencheck convert` |

To try a file without the app:

```sh
cargo run --release -p opencheck-smc --example smc2ifc -- project.smc out-folder
```

## Taking in a new revision of the reader

Open BIM Suite moves fast. To take in a newer `stream.rs` and `mesh.rs`:

1. Copy the two files from `src/loaders/smc/` of the revision over
   `crates/opencheck-smc/src/`, keeping the header comment of each file and
   putting back the lines marked `OpenCheck:` (see
   `licenses/open-bim-suite-NOTICE` for what they are).
2. Run `cargo fmt -p opencheck-smc`.
3. Change `READER_REVISION` and `READER_REVISION_DATE` in `lib.rs`, the
   revision in `licenses/open-bim-suite-NOTICE` and in this file.
4. Copy their `LICENSE` over `licenses/open-bim-suite-LICENSE` in case it
   changed.
5. Run `cargo test -p opencheck-smc`, and with a real file
   `OPENCHECK_SMC=path\to\project.smc cargo test -p opencheck-smc`.
6. Convert a file and compare with the previous result, with the example or
   in the app.
