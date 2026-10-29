# Information requirements

An information rule checks that building parts carry the information their
level of development (LOD) requires: classification, type, dimensions,
storey and so on. Unlike clash rules it looks at one element at a time.

## Levels

OpenCheck ships one standard, `opencheck.information.lod-dk`, with the Danish
levels LOD 200 DK, LOD 300 DK and LOD 325 DK. Each kind of
building part (wall, door, cable carrier, duct, ...) lists the information
it needs at each level.

The **project** is checked at one level, chosen on the card of the
"Information: LOD DK" ruleset in the Check tab (default LOD 300 DK). A
**model** can use another level: open the ⋮ menu on its card in the model
list and choose *Information level*. Levels are saved in the project.

## How information is found

Projects keep the same information in different places. A classification
code may be an `IfcClassificationReference`, a `CCSClassCode` property in a
company property set or `BIM7AATypeCode` in a `BIM7AA` set. An item of
information therefore lists several *sources*; it is present when any of them
has a value. Empty texts do not count as values.

| Source | Present when |
|---|---|
| `property = "<pattern>"` | a property or quantity whose name matches has a value (instance or type) |
| `pset = "<pattern>"` | (with `property`) only in property sets whose name matches |
| `attribute = "<pattern>"` | a direct IFC attribute (`Name`, `ObjectType`, `LongName`, `OverallWidth`, ...) has a value |
| `type_name = true` | the element has a type with a name |
| `classification = true` | the element or its type has a classification reference |
| `material = true` | the element or its type has a material |
| `system = true` | the element is assigned to a system |
| `storey = true` | the element is contained in a building storey |
| `geometry = true` | the element has geometry (levels and positions follow from it) |

Patterns are regular expressions matched without regard to case. Names are
also tried without spaces and underscores, so `^InsulationThickness$` finds
Revit's "Insulation Thickness".

### Measured dimensions

Many exports leave an element's dimensions out: Revit, for one, writes its
length, width and height only when base quantities are exported, yet the
geometry and the material layers carry them. OpenCheck therefore measures
every element with geometry when it loads a model and adds the quantity set
**Dimensions (measured by OpenCheck)**, in the model's length unit:

| Quantity | Measured as |
|---|---|
| `LayerThickness` | the total of the material layers of a layered wall, slab or roof (from the element or its type) |
| `Thickness` | the smallest of length, width and height |
| `Length`, `Width` | the sides in plan of the smallest box around the element, turned to lie along it; length is the longer |
| `Height` | the vertical extent |

An assembly without geometry of its own, such as a curtain wall, is measured
by its parts. The built-in standard's dimension items (Tykkelse, Højde,
Længde, Bredde, Dimension) find these quantities by their names, so the
results say where a value came from, e.g. *found in Dimensions (measured by
OpenCheck).LayerThickness*. The set shows with the element's properties
like any other; a project that wants only stated dimensions can leave the
set out under *Where to look*.

## Adapting where items are found

Projects name things differently, and a source can also pick up the wrong
parameter. On the card of the "Information: LOD DK" ruleset, *Where to
look…* opens an editor:

- **Items** on the left; a pencil marks items the project has changed.
- **Required at** is a table of the building parts that need the item and
  at which LOD levels (the project's level is highlighted). A row selects
  the part: a part can have its own sources for an item, e.g. a width that
  cable carriers keep in another property than doors. A part without its
  own sources uses the item's; *All building parts* edits the item's.
- **Sources** are the places the item is read from: property (with an
  optional property set), IFC attribute, type name, classification,
  material, system, storey or geometry. Names are matched exactly (case,
  spaces and underscores do not matter); tick *Pattern* to write a regular
  expression instead.
- **Last check** shows where the item was found on how many elements, so a
  wrong parameter stands out.
- **Elements** (right) lists the elements the item is checked on, missing
  ones first, with how many have it per model. Pick one, or the element
  selected in the 3D view, to see its property sets and attributes with
  values; the one the item was found in is marked. + adds a property as a
  source. *Show in 3D* selects and frames the element.
- **All properties** counts every property and attribute the elements of the
  part carry; + adds one as a source.
- Several sources are **alternatives**: when consultants keep the same
  information in different places, add each place and every model passes.
  The per-model counts show when all files are covered. *Not applied yet*
  marks edits that are not in use; after *Apply* the element list updates.
- *Apply* takes effect from the next check; *Back to standard* removes the
  change.

The fill rates in the results show the same "found in" list when you hover
over an item; a click opens the editor on that item and part. The command
line prints the most common places under each item.

The changes are saved in the project as `requirements/<standard id>.toml`
(see [file-format.md](file-format.md)). Only the changes are stored, so
improvements to the built-in standard still reach the project:

```toml
base = "opencheck.information.lod-dk"

# For all building parts.
[items]
classification-code = [{ classification = true }, { property = "^CCSClassCode$" }]

# For one part, by part id and item id.
[parts.electrical-carrier]
dimension = [{ property = "^Bredde$", pset = "^EL_Kabelbakke$" }]
```

Standard files can give parts their own sources directly, with the same
shape: `[part.sources]` followed by `item-id = [ ... ]`.

## Building parts

A part applies to the elements any of its selectors match (the same
selectors as in rulesets: `disciplines`, `classes`, `exclude_classes`,
`properties`, `names`, `materials`). An element belongs to the **first**
part that applies, so specific parts come before general ones. Parts of
assemblies (curtain wall panels, stair flights inside a stair) follow their
assembly and are not checked on their own.

## File format

```toml
[standard]
id = "company.information.example"
name = "Example requirements"
levels = ["200", "300"]
default_level = "300"

[[item]]
id = "classification-code"
name = "Classification code"
any = [
    { classification = true },
    { property = "^(CCS|CCI)?(Classification|ClassCode)$" },
]

[[part]]
id = "cable-carrier"
name = "Cable carriers"
applies = [
    { classes = ["IfcCableCarrierSegment", "IfcCableCarrierFitting"] },
    { disciplines = ["electrical"], classes = ["IfcFlowSegment", "IfcFlowFitting"] },
]
[part.levels]
"200" = []
"300" = ["classification-code"]
```

The structure follows the same idea as buildingSMART IDS (an applicability
and requirements per specification), with alternative sources per
requirement added. See below for IDS files.

## Results

The results panel shows the fill rate per building part (elements with all
required information, and per item how many have it) and, per discipline and
building part, every element with what it is missing. Findings can be turned
into issues like any other; an issue then names the missing items.

## IDS files

OpenCheck reads and writes buildingSMART **IDS 1.0** (Information Delivery
Specification) files.

**Import.** In the Check tab, *Add → Import IDS…* stores the file in the
project as `requirements/<name>.ids` and adds a ruleset "IDS: <title>" with
one information rule. The rule refers to the file:

```toml
[[rule]]
id = "ids"
name = "Elements meet the IDS specifications"
type = "information"
severity = "moderate"
standard = "ids:requirements/delivery.ids"
```

Importing a file with the same name again replaces its content. Removing the
ruleset removes the file from the project. On the command line:

```text
opencheck check ARK.ifc KON.ifc --ids delivery.ids
```

**Results.** Each specification is shown like a building part: how many
applicable elements meet all its requirements, and per requirement how many
meet it. Every element that fails a specification is a finding; the category
is the specification name and the details list the failed requirements
(`(other value)` when the information is there but does not match,
`(not allowed)` for prohibited requirements). An element can fail several
specifications. Problems that concern no single element are listed above the
fill rates: a specification whose `minOccurs` is not reached, more applicable
elements than `maxOccurs` allows, a specification written for other IFC
versions than the models (it is checked anyway: in IDS the version is
metadata), or an IDS file missing from the project. The project's LOD level
does not apply to IDS rules.

**What is evaluated.** All facets and restrictions of IDS 1.0 are read:
entity (name, predefined type), attribute, property (property set, base name,
data type, value), classification (system, value), material and partOf;
`simpleValue`, enumeration, pattern, bounds and length restrictions; and the
cardinalities required, optional and prohibited. Applicability facets must all
match. Values are compared as IDS prescribes:

- Texts, class names and predefined types are case-sensitive. Classes are
  written in upper case (`IFCWALL`; `IfcWall` matches nothing), and so are
  predefined types from an enumeration; user-defined types match exactly.
- Reals match within 1e-6 + 1e-6 × |expected|, so `1` matches 0.999998 and
  `0` matches 0.000001. Bounds (`minInclusive`, ...) are exact.
- Integers match only integer values (`42`, not `42.0`); booleans only
  `true` and `false` in lower case. Patterns match texts only, never numbers.
- A property facet must hold for every property its base name matches, in
  every property set its property set name matches; a matching set without
  the property fails the requirement. This holds for `.*` too: a property set
  given as `.*` matches every set of the element, so each of them must have
  the property. Use a simple value or a narrower pattern (`Pset_.*`) to name
  the sets that must carry it.

A specification with `maxOccurs="0"` inverts its requirements, and without
requirements forbids its applicable elements.

**Limits.** OpenCheck evaluates IDS against what it reads from the model, so
some details differ from a full IFC toolkit:

- Only elements are checked: building elements, spaces and the like.
  Sites, buildings, storeys and the project are spatial nodes, never
  applicable. Type objects, processes, materials and other resources
  (`IFCWALLTYPE`, `IFCTASK`, `IFCMATERIAL`, `IFCSURFACESTYLE...`) are not
  checked either, so specifications for them find no applicable elements.
- Elements are read from the spatial structure and from geometry. An element
  that is neither contained in the structure nor has geometry is not loaded.
- Entity: the exact class is compared, as IDS requires (`IFCWALL` does not
  match `IFCWALLSTANDARDCASE`). In addition, generic IFC2x3 elements such as
  `IfcFlowSegment` also match the class their type implies
  (`IfcPipeSegmentType` → `IFCPIPESEGMENT`). The predefined type is the
  element's own; `USERDEFINED` uses `ObjectType`. A predefined type set only
  on the type object is not seen.
- Attribute: direct attributes with a plain value (text, number,
  enumeration) and `GlobalId`. Empty texts count as no value, so an optional
  attribute that is an empty text passes. Attributes that refer to other
  objects or lists are not read. Values are not converted to SI units.
- Property: a property set on the element replaces the type's set of the same
  name. Length measures (`IfcLengthMeasure`, `IfcPositiveLengthMeasure`,
  `IfcQuantityLength`, ...) are converted from the model's length unit to
  metres; areas, volumes and other measures are compared as stored. Quantity
  types count as their measure (`IfcQuantityArea` = `IFCAREAMEASURE`). Of a
  bounded value only the lower and upper bounds are compared, not the set
  point; table values and predefined property sets
  (`IfcDoorPanelProperties`, ...) are not read.
- Classification: the system name and the code (`Identification` /
  `ItemReference`) of references on the element or its type. Parent
  references in a classification hierarchy are not followed, so a reference
  whose source is another reference has no system, and `EF_25_10` does not
  match a reference `EF_25_10_25` below it.
- Material: the names of the materials themselves (`IfcMaterial.Name`), also
  inside layer, profile and constituent sets and lists. The names of sets,
  layers, profiles and constituents and all material categories are not
  compared.
- partOf: `IFCRELAGGREGATES` (any level up, and from a space up through the
  spatial structure to the project), `IFCRELCONTAINEDINSPATIALSTRUCTURE` (the
  container of the element or its assembly, and the containers above it; the
  project is never a container) and `IFCRELASSIGNSTOGROUP` for systems.
  Other groups (`IFCGROUP`, `IFCINVENTORY`, ...) are not read, and for
  systems only the class can be confirmed, not a predefined type. An element
  contained in a space counts as contained in the space's storey.
  `IFCRELNESTS`, `IFCRELVOIDSELEMENT` and `IFCRELFILLSELEMENT` are not read,
  so such facets are never met.
- `totalDigits`, `fractionDigits` and `uri` are ignored. Patterns are XML
  Schema regular expressions; character class subtraction is not supported
  and makes the file fail to load.

**Conformance.** Against the buildingSMART IDS 1.0 test cases
(`Documentation/ImplementersDocumentation/TestCases`, October 2026),
OpenCheck gives the expected result in 270 of 334 cases: ids 12/12,
tolerance 36/36, property 73/82, entity 28/33, classification 23/27,
restriction 20/25, partof 23/34, attribute 38/56, material 17/29. The
remaining cases fall under the limits above. The test files are bare
entities without spatial structure or geometry, which OpenCheck does not
load; the test (`official_test_cases` in `check/ids/tests.rs`, run with
`IDS_TESTCASES` set to the folder and `--ignored`) attaches them to the
project first.

**Export.** *Add → Export LOD requirements as IDS…* writes the built-in
standard at the project's level. On the command line, all levels or chosen
ones:

```text
opencheck export-ids lod-300.ids --level 300
```

Every building part and level becomes a specification named
"<part> – LOD <level>" (one per selector when a part has several). The TOML
format says more than IDS can; the command lists what was lost:

- **First part wins.** In IDS every specification applies independently, so
  an element several parts apply to is checked against all of them.
- **Alternatives between kinds.** IDS has no "any of" across facets. An item
  becomes one requirement of the first kind IDS can express; sources of
  other kinds are left out. Example: *Klassifikationskode* becomes a
  classification requirement, and the CCS/BIM7AA property alternatives are
  dropped. Alternatives of the same kind are merged into one pattern
  (`baseName` matches any of the property names).
- **Property sets.** In IDS every property set that matches `propertySet`
  must contain the property, so "in any property set" cannot be written.
  Property sources without `pset` are therefore left out, and the item falls
  back to another kind of source if it has one (*Dimension* becomes the
  `OverallWidth`/`OverallHeight` attributes). Add `pset` to a source to
  export it. Merged alternatives with property sets lose the pairing of set
  and name.
- **Patterns.** OpenCheck patterns ignore case and match anywhere; they are
  written as XSD patterns with `[aA]` classes and `.*` at unanchored ends.
  Word boundaries (`\b`) are dropped. An anchored plain name (`^Name$`)
  becomes an exact, case-sensitive simple value, and the spelling without
  spaces and underscores is no longer tried.
- **Not expressible.** `type_name` and `geometry` sources, so items that
  rely only on them (e.g. *Kote*, *Placering*) are left out. Disciplines
  are mentioned in the specification description only. Name selectors match
  `Name` and `ObjectType`, not the type name.
- Classes are written as an enumeration of the class and all its subtypes in
  IFC2X3, IFC4 and IFC4X3, with excluded classes removed.
