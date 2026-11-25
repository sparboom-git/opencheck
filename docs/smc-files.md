# Coordination files (.smc, WIP)

OpenCheck can add a coordination file (`.smc`), the file a model checker
saves with the IFC models of a project put together. Each model in
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

Open BIM Suite and OpenCheck are not affiliated with the maker of any model
checker.

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

## Writing a coordination file (WIP)

OpenCheck can also write a coordination file: the models of a project put
together as one `.smc` that the checker opens, for projects where the
federated model has to be delivered in that form. It is made without the
checker. In the desktop app it is *File → Export → Coordination file
(.smc)…*, offered while **WIP** is on; on the command line it is `opencheck
export-smc`. It has been opened in one version of the checker (25.12).

Each model of the project becomes a model in the file, under the name and
discipline it has in OpenCheck, and with the short name of its discipline
(`A`, `K`, `V`, a second of the same `V2`). Hidden models are written too.
The file is put together in the background; a message says when it is
written. Written per model:

- the project, a site, a building and the storeys with their names and
  elevations;
- every element as the checker's class for its IFC class (`IfcWall` as a
  wall, `IfcPump` as a pump), with GlobalId, name, object type, predefined
  type, tag and description, on its storey;
- its property sets, with texts, numbers, whole numbers and yes/no values;
- its shape as faces of three points in millimetres, with its colour, or a
  colour per triangle where it has several;
- its type, by name and IFC class (`IfcWallType`), one type object for the
  elements that share it;
- its materials, by name;
- the systems it belongs to, by name, each with its elements;
- its classification codes: a classification system per name for the whole
  file, with its codes and the elements of each.

What to be careful about:

- **Shapes are triangles**, the ones OpenCheck works with. The checker's own
  shapes (a profile pulled along a line, a shape cut from another) are not
  written, so what the checker measures from them (a wall's thickness, a
  beam's profile) it has to take from the triangles.
- **The file is larger than one the checker saves of the same IFC files.**
  The checker keeps a wall as a profile and a length and lets equal elements
  share one shape; OpenCheck has only the triangles, and writes each element's
  own. A shape is written once, as its faces: the copy of triangles and
  normals the checker draws from is left empty, and the checker works it out
  when it opens the file. That keeps a file at less than half of what it would
  be, and for one project of four models at 110 MB against the checker's own
  80 MB.
- **Types, materials and systems are written by name**, which is what
  OpenCheck keeps of them: a type has no properties and no GlobalId of its
  own (it gets a new one), a material no layers, thicknesses or category,
  and a system is a plain system whatever kind it was (distribution system,
  zone, group). A code on a type is on the elements of the type.
- **Not written**: openings and what fills them, the link to the IFC file a
  model came from, rulesets, results, issues and presentations. A model
  cannot be updated in the checker from a new IFC file.
- **Numbers in property sets** are written as plain numbers, without a unit.
- **Storeys get new GlobalIds**; the elements keep theirs.
- **An IFC class the checker has no class of its own for** (in the table, see
  below) is written as a plain object; the command says which.
- **Disciplines**: a model is filed under the checker's discipline for
  OpenCheck's (fire protection as sprinkler, infrastructure as road, since the
  checker has roads, bridges, railways and ports and no one discipline for
  them all). A model OpenCheck knows no discipline for is filed as
  architectural.
- **Size**: the model data of one file must stay under 4 GB.

Deliver the IFC files with it. The coordination file is a view of them, not a
replacement.

### How the format was found

A coordination file is the checker's own objects written out by Java's
serialization, a public format. What the objects hold was read out of files
the checker saved of OpenCheck's own small test models, and tried by opening
changed copies in the checker: repacked, with a shape changed, a model taken
out, an element added. The checker's program was not opened or decompiled,
and must not be: only files it saved are looked at.

The classes with their version numbers and fields, the class each IFC class
is written as, and what the fields of a class hold by default are in
`crates/opencheck-smc/src/write/classes.txt`. That table is drawn out of
three files saved by the checker (one with an element of every IFC4 element
class, one with a type of every type class, materials, systems and
classifications) by `testdata/smc-test/make_class_table.py`; the test files and the tools that
read them are kept outside the repository. A checker of another version
reads the file as long as its classes keep their version numbers, which they
did between 25.9 and 25.12.

## Command line

```sh
opencheck convert project.smc                      # writes project.opencheck next to it
opencheck convert project.smc C:\Checks\project.opencheck
```

The project has one IFC per model, with its discipline and hash, and no
rulesets yet; open it in the app or give its models to `opencheck check`.

```sh
opencheck export-smc A.ifc K.ifc V.ifc --output project.smc
```

Each IFC file becomes a model of `project.smc`, in the order given.

## Where the code is

| Where | What |
|---|---|
| `crates/opencheck-smc/src/stream.rs`, `mesh.rs` | The reader, from Open BIM Suite (each change marked `OpenCheck:`) |
| `crates/opencheck-smc/src/ifc.rs` | The models as IFC4 files |
| `crates/opencheck-smc/src/lib.rs` | `convert_file`, the summary, the credit constants |
| `crates/opencheck-smc/src/write/java.rs` | Java objects written as a stream |
| `crates/opencheck-smc/src/write/classes.rs`, `classes.txt` | The classes of the format |
| `crates/opencheck-smc/src/write/build.rs` | The objects of a file, from the models |
| `crates/opencheck-smc/src/write/mod.rs` | `write_smc` and what it takes |
| `crates/opencheck-smc/src/write/models.rs` | OpenCheck's models as what `write_smc` takes |
| `crates/opencheck-smc/licenses` | The licence and notice of the reader |
| `crates/opencheck-app/src/smc.rs` | The credit in the WIP part of Settings; *File → Export* as a coordination file |
| `crates/opencheck-app/src/wip.rs` | The WIP switch, the feature's entry and the conversion window |
| `crates/opencheck-app/src/jobs.rs` | `load_smc_file`: converts in the background and loads the models; `export_smc`: writes one |
| `crates/opencheck-cli/src/main.rs` | `opencheck convert` |
| `crates/opencheck-cli/src/export_smc.rs` | `opencheck export-smc` |

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
