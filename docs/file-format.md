# The `.opencheck` project file

Format version: **1**

An OpenCheck project is a ZIP archive with the extension `.opencheck`. It
contains the IFC files of the project unchanged, the rulesets, IDS files, and
the view state of the application. Any ZIP tool can open it, and everything
inside is either IFC, JSON, TOML or IDS (XML), so other software can read and
write projects without OpenCheck.

## Layout

```text
manifest.json          required; describes the project
models/<name>.ifc      one entry per IFC file, byte for byte as imported
rulesets/<name>.toml   rulesets used by the project
requirements/<name>.ids  optional; IDS 1.0 files that information rules refer to
requirements/<id>.toml   optional; the project's changes to a built-in information standard
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
  "rulesets": ["rulesets/clash.toml", "rulesets/ids-delivery.toml"],
  "requirements": ["requirements/delivery.ids"]
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
| `requirements` | Optional. Archive paths of IDS files, stored as imported, and of changes to a built-in information standard (`requirements/<standard id>.toml`, see [information-requirements.md](information-requirements.md)). An information rule refers to an IDS file with `standard = "ids:requirements/<name>.ids"`. |

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
  ],
  "level": "300",
  "model_levels": [{ "model": 1, "level": "325" }]
}
```

`models` lists positions in `manifest.json`; `null` means all models.
`level` is the information level (LOD) the project is checked at; omitted
means the standard's default. `model_levels` lists models, by position, that
use another level than the project. `area` (optional) is the working area:
a rectangle in plan, `{ "center": [x, y], "half": [hx, hy], "angle": radians }`
in world coordinates (metres); only elements whose extent touches it are
shown and checked.
A finding id is a UUID derived from the ruleset id, the rule id and the
GlobalIds of the two elements, so the same problem gets the same id in every
run and decisions survive re-running a check and updating models. Decisions
are `accepted` (a real problem to handle) or `rejected` (not a problem).

`ratings` (optional) holds suggested C402 categories, from the pre-sorting or
from Claude, by finding id; `triage` (optional) holds the project's triage
rules and the reviewer's corrections of suggestions. See
[claude-triage.md](claude-triage.md).

```json
{
  "ratings": [
    { "finding": "3f2a9c1e-…", "category": "C1", "noise": true, "confidence": 0.95,
      "reason": "Isolering på varmerør VF 3 mod eget rør.", "source": "claude",
      "approved": "C1", "group": "a1b2c3d4e5f6", "at": "2026-10-26T10:00:00Z" }
  ],
  "triage": {
    "rules": "Insulation against pipes in shafts is always C1.",
    "examples": [
      { "finding": "IfcDuctSegment \"Rund Kanal\" (VI 2) vs IfcBeam \"HEB300\", 120 mm, E2",
        "suggested": "C1", "category": "C2", "reason": "…" }
    ]
  }
}
```

`category` is `C1`, `C2` or `C3`; `source` is `presort` or `claude`;
`approved` is the category the reviewer approved it as, absent while it is
only suggested.

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
      "findings": [
        {
          "finding": "3f2a9c1e-7b4d-5e8f-9a01-23456789abcd",
          "ruleset": "Clash: MEP vs structure",
          "rule": "MEP components must not intersect structural elements",
          "kind": "Clash",
          "magnitude_mm": 193.0,
          "ruleset_id": "opencheck.clash.mep-vs-structure",
          "rule_id": "mep-vs-structure",
          "revisions": [
            { "model": "Building-Hvac", "revision": 1 },
            { "model": "Building-Structural", "revision": 1 }
          ],
          "details": [],
          "solved_by_update": {
            "date": "2026-10-20T09:30:00Z",
            "models": [{ "model": "Building-Hvac", "revision": 2 }]
          }
        }
      ],
      "components": [
        { "global_id": "2O2Fr$t4X7Zf8NOew3FLOH", "name": "building element", "ifc_class": "IfcDuctSegment", "model": "Building-Hvac", "role": "a" },
        { "global_id": "3Fbgsvr8nAYfGs9y5keub0", "name": "house - chimney", "ifc_class": "IfcChimney", "model": "Building-Structural", "role": "b" }
      ],
      "point": [-25.1, 26.4, 4.9],
      "camera": { "eye": [-30.2, 21.3, 8.0], "yaw": 0.785, "pitch": -0.55 },
      "section": { "height": 5.15, "anchor": [-25.1, 26.4, 4.9] },
      "show_context": true,
      "hidden": [{ "global_id": "1AQAupaRP1txwK1AGiN61V", "model": "Building-Architecture" }],
      "solved_by_update": true,
      "responsible": ["mechanical", "structural"],
      "assigned_to": "ventilation@example.com",
      "bcf": {
        "project_id": "1234",
        "topic_guid": "8c1d2f3e-…",
        "synced": "2026-10-20T10:00:00Z",
        "remote_status": "Open",
        "remote_assigned_to": "ventilation@example.com",
        "sent_version": "2026-10-20T09:58:12Z"
      }
    }
  ],
  "people": ["ABC", "ventilation@example.com"],
  "report": { "project": "Example School", "author": "ABC", "company": "Example", "revision": "A",
              "disciplines": "Ventilation", "phase": "Projektforslag", "method": "", "remarks": "" },
  "bcf": {
    "server": "https://node1.field.dalux.com/service/",
    "project_id": "1234",
    "project_name": "Example School",
    "assignees": [{ "discipline": "mechanical", "user": "ventilation@example.com" }]
  }
}
```

An issue bundles one or more findings (`findings`) and the components they
involve; issues can be merged. Files written before bundling had a single
`source` object instead of `findings`; readers move it into the list.
`status` is `open`, `in_progress`, `resolved` or `closed`; `priority` is
`critical`, `moderate` or `low`. Components are found again by `global_id`
(and `model` when the same GlobalId appears in several models). `point` and
`camera.eye` are world coordinates in metres. The viewpoint (`camera`, `section`,
`show_context`, `hidden`) is the view the reviewer had when the issue was
created or last updated: `show_context` false means the rest of the model is
ghosted around the components, `section` is a horizontal cut (everything above
`height` removed), and `hidden` lists elements that were switched off.

For missing information, `details` lists the items that were missing and
the issue has one component (role `a`).

`priority` is shown as the C402 criticality: `low` = C1 (not critical),
`moderate` = C2 (critical), `critical` = C3 (very critical). `people` lists
assignees used before (initials or e-mail), offered again in other issues;
`report` holds the header fields of the issue report.

Responsibility: `responsible` lists the disciplines that must solve the
issue; new issues get the disciplines of the models their findings come
from, and the user can add or remove disciplines. `assigned_to` is a person
on the BCF server. The top-level `bcf` object is the BCF project the issues
are shared through, with a default person per discipline (`assignees`) for
issues without their own `assigned_to`. An issue's `bcf` object links it to
its topic on that server; `sent_version` is the issue's `modified` time when
it was last sent, so changed issues can be found. Sign-in tokens are never
stored in the project; see [bcf-live.md](bcf-live.md).

Model updates: each finding records the model `revisions` it was seen in. When
a check after a model update no longer finds it, although its rule
(`ruleset_id`, `rule_id`) ran on the same models and at least one of them has a
newer revision, the finding is not deleted but gets `solved_by_update` with the
date and the newer model versions. When every finding of an issue is solved
this way, the issue gets `"solved_by_update": true` and status `resolved`. A
finding that shows up again loses the mark and its issue is reopened. Files
without these fields are read as if every model was at revision 1.

## Rulesets

Ruleset files are TOML, described in [ruleset-authoring.md](ruleset-authoring.md).
