# The `.opencheck` project file

Format version: **1**

An OpenCheck project is a ZIP archive with the extension `.opencheck`. It
contains the IFC files of the project unchanged, the rulesets, and the view
state of the application. Any ZIP tool can open it, and everything inside is
either IFC, JSON or TOML, so other software can read and write projects
without OpenCheck.

## Layout

```text
manifest.json          required; describes the project
models/<name>.ifc      one entry per IFC file, byte for byte as imported
rulesets/<name>.toml   rulesets used by the project
view.json              optional; camera, hidden elements, section, measurements
checking.json          optional; ruleset settings and decisions on findings
issues.json            optional; selected issues (BCF-style topics)
```

Entries are compressed with Deflate. Readers must not depend on the order of
entries; writers put `manifest.json` last, so its presence means the archive
was written completely.

Directories reserved for later versions: `results/` (checking results, planned
as BCF 3.0 topics) and `cache/` (derived data that can always be deleted).
Readers must ignore entries they do not know.

## manifest.json

```json
{
  "format": "opencheck-project",
  "format_version": 1,
  "generator": "OpenCheck 0.1.0",
  "created": "2026-10-19T10:15:10Z",
  "modified": "2026-10-19T10:15:10Z",
  "models": [
    {
      "file": "models/ARK.ifc",
      "name": "ARK",
      "discipline": "architectural",
      "visible": true,
      "source_path": "C:\\Projects\\House\\ARK.ifc",
      "sha256": "8790a1e1…",
      "revision": 1,
      "added": "2026-10-19T10:13:41Z",
      "updated": "2026-10-19T10:13:41Z"
    }
  ],
  "rulesets": ["rulesets/clash.toml"]
}
```

| Field | Meaning |
|---|---|
| `format` | Always `opencheck-project`. |
| `format_version` | Integer. A reader refuses files with a higher version than it knows. |
| `generator` | Application and version that wrote the file. Informational. |
| `created`, `modified` | RFC 3339 timestamps in UTC. |
| `models` | IFC files in load order. The first model defines the shared origin of the federation. |
| `rulesets` | Archive paths of the ruleset files. |

Model entries:

| Field | Meaning |
|---|---|
| `file` | Archive path of the IFC file. |
| `name` | Short name shown in the application. |
| `discipline` | One of `architectural`, `structural`, `mechanical`, `plumbing`, `electrical`, `fire_protection`, `landscape`, `infrastructure`, `unknown`. |
| `visible` | Whether the model is shown. Defaults to `true`. |
| `source_path` | Path the file was imported from. Informational only; never used to load the model. |
| `sha256` | SHA-256 of the IFC file, lowercase hex. Identifies the model version. |
| `revision` | Starts at 1, increases when the model is replaced by a newer delivery. |
| `added`, `updated` | RFC 3339 timestamps. |

## view.json

Written by the application to restore the session. All coordinates are world
coordinates in metres (the IFC project coordinate system), so they do not
depend on the order models are loaded in.

```json
{
  "camera": { "eye": [-11.3, -10.2, 12.3], "yaw": 0.785, "pitch": -0.55 },
  "xray": false,
  "show_spaces": false,
  "storey_filter": null,
  "section": { "height": 3.005, "anchor": [5.8, 6.1, 0.0] },
  "hidden": [{ "model": 0, "global_id": "1AQAupaRP1txwK1AGiN61V" }],
  "isolated": null,
  "measurements": [
    { "a": [-0.74, 6.69, 0.08], "b": [-2.14, 7.09, 1.78], "kind": "Laser" }
  ]
}
```

Elements are referenced by the model's index in `manifest.json` and the IFC
`GlobalId`, which stays the same when a model is replaced by a new revision.
Unknown fields must be ignored and missing fields take their defaults.

## checking.json

Which rulesets are switched on, which models they run on, and the reviewer's
decisions on findings.

```json
{
  "rulesets": [
    { "path": "rulesets/clash-mep-vs-structure.toml", "enabled": true, "models": null },
    { "path": "rulesets/model-quality-duplicates.toml", "enabled": true, "models": [0, 2] }
  ],
  "decisions": [
    { "finding": "3f2a9c1e-7b4d-5e8f-9a01-23456789abcd", "decision": "accepted" }
  ]
}
```

`models` lists positions in `manifest.json`; `null` means all models.
A finding id is a UUID derived from the ruleset id, the rule id and the
GlobalIds of the two elements, so the same problem gets the same id in every
run and decisions survive re-running a check and updating models. Decisions
are `accepted` (a real problem to handle) or `rejected` (not a problem).

## issues.json

The selected issues: findings the reviewer has picked out to be handled and
reported. Fields follow the BCF 3.0 topic model so issues can be exported to
BCF without loss.

```json
{
  "issues": [
    {
      "guid": "0b8e4f6a-2c1d-4e3f-8a9b-1c2d3e4f5a6b",
      "title": "building element vs house - chimney",
      "description": "",
      "status": "open",
      "priority": "critical",
      "created": "2026-10-19T12:00:00Z",
      "modified": "2026-10-19T12:00:00Z",
      "labels": ["Clash"],
      "source": {
        "finding": "3f2a9c1e-7b4d-5e8f-9a01-23456789abcd",
        "ruleset": "Clash: MEP vs structure",
        "rule": "MEP components must not intersect structural elements",
        "kind": "Clash",
        "magnitude_mm": 193.0
      },
      "components": [
        { "global_id": "2O2Fr$t4X7Zf8NOew3FLOH", "name": "building element", "ifc_class": "IfcDuctSegment", "model": "Building-Hvac", "role": "a" },
        { "global_id": "3Fbgsvr8nAYfGs9y5keub0", "name": "house - chimney", "ifc_class": "IfcChimney", "model": "Building-Structural", "role": "b" }
      ],
      "point": [-25.1, 26.4, 4.9],
      "camera": { "eye": [-30.2, 21.3, 8.0], "yaw": 0.785, "pitch": -0.55 }
    }
  ]
}
```

`status` is `open`, `in_progress`, `resolved` or `closed`; `priority` is
`critical`, `moderate` or `low`. Components are found again by `global_id`
(and `model` when the same GlobalId appears in several models). `point` and
`camera.eye` are world coordinates in metres.

## Rulesets

Ruleset files are TOML, described in [ruleset-authoring.md](ruleset-authoring.md).
