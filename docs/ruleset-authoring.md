# Writing OpenCheck rulesets

This guide describes the ruleset format of OpenCheck, an open source IFC model
checker. It is written so that you can hand it to a language model (ChatGPT,
Claude, Copilot or similar) together with a description of the checks you
need, and get back a ruleset file you can import in OpenCheck
(Checking → Add → Import ruleset file).

> **To the language model reading this:** produce one complete TOML file that
> follows this specification exactly. Use only the keys listed here; unknown
> keys make the file invalid. Explain your assumptions in TOML comments
> (`# ...`) inside the file, not outside it. Reply with the file in a single
> code block.

## What OpenCheck checks

OpenCheck loads several IFC models (typically one per discipline, e.g.
architecture, structure, ventilation) into one federated model. A ruleset is
a list of rules. Each rule selects two groups of elements, **a** and **b**,
and tests every pair of an element from **a** and an element from **b**:

| Rule type | Reports a pair when |
|---|---|
| `clash` | the solids intersect deeper than `tolerance_mm`, or one lies completely inside the other |
| `clearance` | the elements come closer than `clearance_mm` (touching and intersecting included) |
| `duplicates` | two elements have the same shape in the same place within `tolerance_mm`, whatever their IFC class or tessellation |
| `information` | (one element at a time) a building part lacks information that an information standard requires at the project's LOD level; see [information-requirements.md](information-requirements.md) |

Each reported pair is a *finding*. Users review findings, accept or reject
them, and turn the relevant ones into issues.

## File structure

A ruleset is a TOML file with one `[ruleset]` table and one or more `[[rule]]`
tables.

```toml
[ruleset]
id = "company.clash.sprinkler-vs-structure"   # required, unique, no spaces
name = "Sprinklers vs structure"              # required, shown in the UI
description = "Sprinkler pipes must not run through beams or columns."
version = "1"                                 # optional, default "1"
author = "Jane Doe, Example Engineering"      # optional

[[rule]]
id = "sprinkler-pipes-vs-beams"               # required, unique within the file
name = "Sprinkler pipes must not intersect beams or columns"
description = "Optional longer explanation shown to the user."
type = "clash"                                # clash | clearance | duplicates | information
severity = "critical"                         # critical | moderate | low
tolerance_mm = 10                             # clash and duplicates; default 10
between = "different_models"                  # any | different_models | same_model

[rule.a]
disciplines = ["fire_protection"]
classes = ["IfcPipeSegment", "IfcPipeFitting"]

[rule.b]
disciplines = ["structural"]
classes = ["IfcBeam", "IfcColumn"]
```

### `[ruleset]`

| Key | Required | Meaning |
|---|---|---|
| `id` | yes | Stable identifier. Use reverse-domain style, e.g. `acme.clash.mep-vs-structure`. Changing it makes previous decisions on findings unmatched. |
| `name` | yes | Short name shown in the ruleset list. |
| `description` | no | One or two sentences on purpose and scope. |
| `version` | no | Free text; increase when the rules change. |
| `author` | no | Person or organisation. |

### `[[rule]]`

| Key | Required | Meaning |
|---|---|---|
| `id` | yes | Unique within the ruleset. Keep it stable: findings are matched between runs by ruleset id, rule id and the two elements. |
| `name` | yes | Shown in the results list. Phrase it as the requirement ("Ducts must not intersect beams"). |
| `description` | no | Explanation for the reviewer. |
| `type` | yes | `clash`, `clearance`, `duplicates` or `information`. |
| `severity` | yes | `critical`, `moderate` or `low`. |
| `tolerance_mm` | no | Clash: intersections up to this depth are ignored (default 10). Duplicates: largest offset between two copies (default 10). |
| `clearance_mm` | for `clearance` | Required free distance; must be above 0. |
| `between` | no | `any` (default), `different_models` or `same_model`. Use `different_models` for coordination between disciplines and `same_model` for model quality checks. |
| `standard` | for `information` | Id of the information standard, e.g. `opencheck.information.lod-dk`. Group `a` then limits which elements are checked; `b` is not used. |
| `a` | no | Selector for the first group. Omitted: every element. |
| `b` | no | Selector for the second group. **Omitted: elements of `a` are tested against each other.** |

### Selectors: `[rule.a]` and `[rule.b]`

An element belongs to a group when it matches **all** keys given:

| Key | Meaning |
|---|---|
| `disciplines` | List of model disciplines (see below). Empty or omitted: any. The discipline is set per model by the user. |
| `classes` | List of IFC classes. **Subtypes match too**: `IfcWall` also matches `IfcWallStandardCase`, `IfcBuildingElement` matches all walls, slabs, beams and so on. IFC2x3 elements also match the class their type gives them: an `IfcFlowSegment` with an `IfcDuctSegmentType` matches `IfcDuctSegment`. Empty or omitted: any class. |
| `exclude_classes` | IFC classes to leave out, subtypes included. |
| `properties` | List of property conditions; all must hold. |
| `names` | Regular expressions (case is ignored) for the element's name, object type or type name; one must match. |
| `materials` | Regular expressions for material names; one must match a material of the element. |

Property condition:

```toml
[[rule.b.properties]]
pset = "Pset_WallCommon"   # property set name; "" or omitted = any set
name = "LoadBearing"
equals = "true"            # optional; omitted = the property must exist
```

Comparison is case-insensitive. `true` / `false` match boolean properties,
numbers are compared as numbers.

### Disciplines

`architectural`, `structural`, `mechanical` (shown as Ventilation, code VEN),
`plumbing` (water and drainage), `electrical`, `fire_protection` (shown as
Sprinkler, code SPR), `landscape`, `infrastructure`, `unknown`.

### Useful IFC classes

Matching uses the IFC inheritance, so a broad parent class is often the most
robust choice; it works across IFC2x3, IFC4 and IFC4x3.

| Purpose | Classes |
|---|---|
| All building parts | `IfcBuildingElement` (IFC4x3 name `IfcBuiltElement` is accepted as well) |
| Structure | `IfcBeam`, `IfcColumn`, `IfcSlab`, `IfcWall`, `IfcFooting`, `IfcPile`, `IfcMember`, `IfcPlate`, `IfcRoof` |
| Openings and access | `IfcDoor`, `IfcWindow`, `IfcStair`, `IfcStairFlight`, `IfcRamp`, `IfcRailing` |
| Finishes | `IfcCovering`, `IfcCurtainWall` |
| All MEP | `IfcDistributionElement` |
| MEP that carries flow | `IfcDistributionFlowElement` |
| Ducts | `IfcDuctSegment`, `IfcDuctFitting`, `IfcAirTerminal` (IFC2x3: `IfcFlowSegment`, `IfcFlowFitting`, `IfcFlowTerminal`) |
| Pipes | `IfcPipeSegment`, `IfcPipeFitting` (IFC2x3: `IfcFlowSegment`, `IfcFlowFitting`) |
| Cable trays | `IfcCableCarrierSegment`, `IfcCableCarrierFitting` |
| Equipment | `IfcFlowController`, `IfcEnergyConversionDevice`, `IfcFlowMovingDevice` |
| Furniture | `IfcFurnishingElement` |
| Placeholders | `IfcBuildingElementProxy` (often survey markers or generic objects; frequently excluded) |

Spaces (`IfcSpace`) and openings are not solid building parts; openings are
never checked.

## Examples

### MEP must not run through structure

```toml
[ruleset]
id = "example.clash.mep-vs-structure"
name = "MEP vs structure"

[[rule]]
id = "mep-vs-structure"
name = "MEP must not intersect structural elements"
type = "clash"
severity = "critical"
tolerance_mm = 10
between = "different_models"

[rule.a]
disciplines = ["mechanical", "plumbing", "electrical"]
classes = ["IfcDistributionElement"]

[rule.b]
disciplines = ["structural"]
classes = ["IfcBuildingElement"]
exclude_classes = ["IfcBuildingElementProxy"]
```

### Keep 100 mm free around ducts near load-bearing walls

```toml
[ruleset]
id = "example.clearance.ducts"
name = "Clearance around ducts"

[[rule]]
id = "ducts-vs-load-bearing-walls"
name = "Keep 100 mm between ducts and load-bearing walls"
type = "clearance"
severity = "low"
clearance_mm = 100
between = "different_models"

[rule.a]
classes = ["IfcDuctSegment", "IfcDuctFitting"]

[rule.b]
classes = ["IfcWall"]

[[rule.b.properties]]
pset = "Pset_WallCommon"
name = "LoadBearing"
equals = "true"
```

### Walls modelled twice in the architectural model

```toml
[ruleset]
id = "example.quality.duplicate-walls"
name = "Duplicate walls"

[[rule]]
id = "duplicate-walls"
name = "Walls must not be modelled twice"
type = "duplicates"
severity = "moderate"
tolerance_mm = 5
between = "same_model"

[rule.a]
disciplines = ["architectural"]
classes = ["IfcWall"]
```

## Tips

- One rule per requirement. Several narrow rules give clearer results than one
  broad rule, and each can have its own severity.
- Coordination between disciplines: `between = "different_models"` and a
  discipline on both sides.
- A group checked against itself (no `[rule.b]`) reports each pair once.
- Start with a tolerance of 10 mm for clashes; lower values report touching
  elements from rounding in the authoring tool.
- Keep `id` values stable once a ruleset is in use.

## Prompt template

> I use OpenCheck for IFC model checking. Using the attached specification,
> write a ruleset that checks the following: *(describe the requirements, the
> disciplines involved, and how strict each check is)*. Reply with one TOML
> file.
