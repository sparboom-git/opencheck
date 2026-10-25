# Visual Check

The *Visual Check* tab holds checks that colour the model, so a pattern, or
a break in it, shows at a glance. There are four, each added on its own with
*Add*, because it does not always make sense to run them all:

| Check | Colours | Marks | Check type |
|---|---|---|---|
| Alignment of walls and columns | groups of elements in line | walls and columns out of line | `alignment` |
| Dimensions in pipe and duct systems | each kind of system | pieces larger than the one before | `dimension_jump` |
| Fall of drain pipes | enough fall | flat, wrong way, too little | `drain_slope` |
| Storeys | each storey | elements on another storey, without a storey | `storey_mismatch` |

The checks are kept with the project; *Analyse* works the colours out. The
right panel lists the problems first (what is wrong, with the element under
it) and then the colours. A click selects the element or the group and zooms
to it. Everything the check does not colour is ghosted, so the colours stand
out; *Show the other elements normally* turns that off. The colours stay on
the Visual Check tab, or on all tabs with *Keep the colours on the other
tabs*. *Clear colours* removes them.

**Selection**: a check looks at all elements of its models, or only the
selected ones. Hidden elements and elements outside the working area are
left out, so a working area limits a check to a part of the building.

**Autorun**: the checks ticked in the list run with autorun too. Their
problems are kept as findings, with the check type above, the elements, the
disciplines, a severity and the measured value, so they appear in the
history and the dashboard like the findings of rulesets. See
[autorun.md](autorun.md).

## Alignment of walls and columns

Checks that structural walls and columns stand in line from storey to
storey. Elements in line with each other form a group, and every group gets
a colour of its own:

- **Same colour**: in line with each other, within the tolerance.
- **Red**: out of line. A wall or column stands above or below it, but it
  is in line with none of them.
- **Grey**: nothing above or below to line up with, such as columns of a
  single storey.

Settings:

- **Models**: the disciplines whose models are looked at, *Structural* by
  default. The discipline of a model is set in the Model tab.
- **Elements**: walls (`IfcWall…`) and columns (`IfcColumn…`). Walls are
  compared with walls, columns with columns.
- **Check alignment**:
  - *Centerline alignment* compares the centre lines of walls and the
    centre points of columns.
  - *Face alignment* compares the faces of walls: a thinner wall flush with
    one face of the wall below counts as in line. Columns are still compared
    by their centre points.
- **Tolerance**: the largest offset, in millimetres, that still counts as
  in line. Keep it small (0–3 mm) to catch small shifts. Offsets below half
  a millimetre always count as in line, because of rounding in the model
  geometry.

Each element's footprint in plan is its smallest enclosing rectangle. Two
walls are in line when they run parallel (within 10°), overlap along their
length, and their centre lines (or a pair of faces) are within the
tolerance of each other at both ends of the overlap; a wall turned slightly
against the one below is therefore out of line too. Two columns are in line
when their centre points are within the tolerance. Groups follow from
pairs: a wall in line with the wall below and the wall above joins both in
one group. An element counts as having one above or below when an element
of the same kind is less than a wall thickness (or a column size) away in
plan and does not share more than 50 mm of height with it.

Curved walls are measured by their enclosing rectangle too, so two curved
walls of different length on the same curve can show as out of line.

## Pipe and duct systems

The dimension check and the drain fall check work on the same network of
pipes, ducts, fittings, valves and equipment in the models of the chosen
disciplines (*Ventilation*, *Plumbing* and *Sprinkler* by default).

**Connections.** Elements whose ports are connected (`IfcDistributionPort`
with `IfcRelConnectsPorts`, the ports found through
`IfcRelConnectsPortToElement` or `IfcRelNests`) are connected. Loose
elements without connected ports are connected where the end of a segment
lies within the connection tolerance (10 mm) of the end of another segment
or inside the box of a fitting, within the same system. Both kinds can be
mixed in one model. The result lists how many connections came from each.

**Systems.** An element belongs to the system it is assigned to
(`IfcSystem` or `IfcDistributionSystem` through `IfcRelAssignsToGroup`).
Without one, a system property (*System Name*, *System Type*, *System
Classification*, *Systemnavn*) is used. Elements without either take the
system of the elements they connect to; what is left forms a network of its
own (*Network 1*, ...). Systems are coloured by their kind, from the
system's description or type (`Afløb, Spildevand`, `Ventilation, Tilluft`,
DRAINAGE): cold water blue, heating orange, drains brown, supply air green
and so on, muted, so the problems stand out in red.

**Segments.** The run of a segment is the line between its two ports; for
a segment without ports, the side of its box that points to what it
connects to. Its size is measured across that line from the geometry, so
it works without size properties. Pieces shorter than they are wide (stubs,
couplings) and pieces that are not straight extrusions (silencers, dampers
or flexible ducts exported as segments) are not measured; they are passed
through like fittings.

## Dimensions in pipe and duct systems

A system usually starts large at the shaft or the air handling unit and
gets smaller towards the consumers. The check marks a pipe or duct that is
larger than the piece before it, seen from the start of its system: an
increase such as Ø160, Ø125, Ø160 is most likely a modelling error.

- **Start.** Each connected part of a system starts at its equipment (air
  handling unit, fan, pump, boiler, ...) or, without equipment, at its
  largest pieces. From there the network is followed outwards, and each
  measured piece is compared with the last measured piece before it.
- **Unclear direction.** When the largest pieces of a part lie in more than
  one place, the direction is not clear. There, every piece smaller than
  the pieces on both sides of it along a run (through bends and other
  fittings with two connections, not through tees) is marked: a dip in the
  dimension is suspicious whichever way the flow goes.
- **Size.** Round pipes and ducts by their diameter, rectangular ducts by
  their area (the side of a square of the same area), so 400×200 to
  300×300 is no jump. A reducer is judged by the pieces on both sides of it.
- **Tolerance.** Larger by more than this percentage (5 % by default)
  counts as a jump, so rounding gives no false problems.

The problem names both sizes (`Ø160 after Ø125 in VI 3`) and selects the
larger piece.

## Fall of drain pipes

Colours the drain pipes by their fall and marks pipes lying flat or falling
the wrong way. Other pipes (heating, water), which may well rise, are left
out.

- **Drain systems.** A system is a drain when its name or kind holds one of
  the words in the list (afløb, spildevand, regnvand, kloak, dræn,
  sanitary, drain, waste, sewage, storm, soil); IFC4 predefined types such
  as DRAINAGE, SEWAGE, STORMWATER and WASTEWATER hold them too. The panel
  lists the pipe systems of the models as the check sees them; ticking or
  unticking one sets it by hand.
- **Fall.** Per horizontal pipe: the height difference of its ends over
  its horizontal length, in per mille. Pipes steeper than 45° are stacks and
  are not judged.
- **Direction.** Drains run down to their outlet, so the way through the
  connected drain pipes towards their lowest point is downstream. A pipe
  must fall towards that end. A pipe connected to no other drain pipe is
  judged by the size of its fall only.
- **Least fall.** One value for all pipes (10 ‰ by default), and limits by
  outer diameter (*Limit by pipe size*: up to 110 mm at least 20 ‰, ...).
  Set them to the project's rules (DS 432 and your own practice).
- **Tolerance.** Less fall than this (0.5 ‰) is flat; a fall within it of
  the least fall counts as enough.

Colours: **green** enough fall in the right direction, **yellow** the right
direction but less than required, **red** flat or the wrong way, **grey**
stacks and fittings.

## Storeys

Finds elements that stand on another storey than the one they belong to,
such as a wall or a pipe on the 4th floor contained in the ground floor.

- **Storeys.** Each storey reaches from its own height up to the next
  storey. The height is the storey's placement in the IFC world frame (its
  `ObjectPlacement` followed up to the project), or its *Elevation* when it
  has none, so it matches the geometry also when the building is placed
  above sea level.
- **Where an element stands.** Judged by its underside, also for elements
  going through several storeys (columns, stacks, shafts, stairs).
- **Tolerance.** How far an element may reach below its storey (500 mm by
  default), so slabs, floor build-ups and beams under the deck give no false
  problems.
- **May stand anywhere.** IFC classes that are not judged, e.g. `IfcBeam`
  when beams are modelled on the storey above.

Colours: a muted colour per storey (by name across the models), **red** for
elements standing on another storey (the text says which, and the value is
the offset from the storey's floor), **purple** for elements that belong to
no storey.
