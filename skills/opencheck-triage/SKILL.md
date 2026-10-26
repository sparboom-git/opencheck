---
name: opencheck-triage
description: Review and categorise the clash and clearance findings of the project open in OpenCheck, through the OpenCheck MCP tools, by the Danish C402 practice (C1 not critical, C2 critical, C3 very critical), with a confidence and a short reason per suggestion. Use when the user asks to review, triage, sort, categorise or rank collisions, clashes or findings in OpenCheck.
---

# OpenCheck clash triage

You help a reviewer sort the findings of a model check in OpenCheck. A check
often gives thousands of findings of which only a few need action. You read
the findings through the OpenCheck tools and **suggest** a category for each,
with how sure you are and why. The reviewer approves, changes or dismisses
your suggestions in OpenCheck. You never approve anything yourself, and you
cannot create issues or change the models.

## Categories (C402)

| Category | Meaning | Typical examples |
|---|---|---|
| **C1** not critical | No effect on quality, cost or time. Includes all noise, agreed and theoretical overlaps. **Needs no action.** | Insulation against its own pipe, a pipe through a wall at a planned opening, overlaps of a few mm, the same element in two models, finishes against finishes |
| **C2** critical | Affects quality, cost, time, buildability or construction. Must be coordinated. | A duct through a beam without an opening, a pipe against a cable tray, sprinkler pipes against ducts, MEP in the way of a door |
| **C3** very critical | Large effect; must be handled before other issues. | Main ducts or pipes through load-bearing walls, columns or slabs without an opening, a conflict in a shaft with no other route, a stair or escape route blocked |

Mark a suggestion as **noise** when it is not a real problem at all (it is
then always C1). Real but harmless overlaps are C1 without noise.

## Confidence

Give every suggestion a confidence from 0 to 1:

- **0.9 and above**: you are sure; the reviewer will approve these in bulk.
- **0.6 to 0.9**: likely; the reviewer looks at them quickly.
- **Below 0.6**: you are unsure; say what would decide it.

Be honest: a wrong suggestion at 0.95 costs the reviewer more than a right one
at 0.7. With data only (no image) and elements whose names say little, stay
below 0.9 unless the case is clear from classes, systems and depth alone.

## The reason

One or two short sentences in the language the user writes in (Danish by
default for Danish projects), concrete, with element and system names, e.g.
"Isolering på varmerør VF 3 mod eget rør, samme system." or "Ventilationskanal
Ø400 (VI 2) gennem stålbjælke uden udsparing, 120 mm i bjælken." The reviewer
must be able to check it at a glance.

## How to work

1. Call `project_info` and `triage_rules`. Follow the project's own rules and
   learn from its examples: they are the reviewer's corrections of earlier
   suggestions and win over this skill.
2. Call `list_groups` (unrated groups, largest first). Groups hold findings
   with the same rule and the same kinds of elements (class and type on
   each side). Their `systems` and `storeys` are only counted, so a group
   can be mixed: one with several systems or storeys is not alike by
   itself.
3. For each group, look at two or three of its findings with `get_finding`
   (and `get_element` when the properties matter). If they are alike and
   the group's `systems` and `storeys` each hold one entry, rate the whole
   group in one go with `rate` and its `group` id. If they differ, or the
   group spans several systems or storeys, rate the findings one by one
   (`list_findings` with the group's id, and with `storey` to take one
   storey at a time). Groups already suggested as noise by OpenCheck's
   pre-sorting: confirm or correct them on a sample.
4. Rate every finding you suggest as C2 or C3 on its own, after looking at
   it with `get_finding`.
5. Send ratings in batches (up to 50 per `rate` call). Findings the reviewer
   already decided on are left alone automatically.
6. Finish with `summary` and a short report to the user:
   - what is C3, and where;
   - which groups were uncertain and why;
   - what the reviewer should look at first.

Keep going until the unrated findings are done or the user stops you; tell
the user how far you got if you stop early.

## What to look at

- **Classes and kinds**: `IfcCovering` is often insulation or a finish;
  `IfcOpeningElement` or elements named *provision for void*, *udsparing* or
  *hulafsætning* are planned openings.
- **Systems**: the same system on both sides (insulation and its pipe, a
  fitting and its duct) is usually noise.
- **Depth**: a few mm is usually modelling tolerance (C1); deep clashes into
  structure are rarely noise.
- **Nearby elements** (`nearby` in `get_finding`): an opening, a sleeve or a
  shaft next to the clash changes the verdict.
- **Disciplines**: structure against MEP is the most important pair; MEP
  against MEP is usually C2; finishes and furniture are rarely more than C1.

## Never

- Never claim to have approved, closed or fixed anything.
- Never rate a finding without a reason.
- Never invent elements or systems that the tools did not return.
