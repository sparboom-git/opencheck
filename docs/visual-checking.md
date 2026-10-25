# Visual Check

The *Visual Check* tab holds rules that colour the model instead of
listing findings, so a pattern, or a break in it, shows at a glance. The
rules are kept with the project; the colours are worked out again with
*Analyse*.

## Alignment of walls and columns

Checks that structural walls and columns stand in line from storey to
storey. Elements in line with each other form a group, and every group gets
a colour of its own:

- **Same colour**: in line with each other, within the tolerance.
- **Red**: out of line. A wall or column stands above or below it, but it
  is in line with none of them.
- **Grey**: nothing above or below to line up with, such as columns of a
  single storey.

Everything else is ghosted, so the colours stand out; *Show the other
elements normally* turns that off. The colours stay on the Visual Check
tab, or on all tabs with *Keep the colours on the other tabs*. *Clear
colours* removes them.

### Settings

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
- **Selection**: all walls and columns in the models, or only the selected
  ones. Hidden elements and elements outside the working area are left out,
  so a working area limits the check to a part of the building.

### How it is measured

Each element's footprint in plan is its smallest enclosing rectangle. Two
walls are in line when they run parallel (within 10°), overlap along their
length, and their centre lines (or a pair of faces) are within the
tolerance of each other at both ends of the overlap; a wall turned slightly
against the one below is therefore out of line too. Two columns are in line
when their centre points are within the tolerance. Groups follow from
pairs: a wall in line with the wall below and the wall above joins both in
one group.

An element counts as having one above or below when an element of the same
kind is less than a wall thickness (or a column size) away in plan and does
not share more than 50 mm of height with it.

Curved walls are measured by their enclosing rectangle too, so two curved
walls of different length on the same curve can show as out of line.

### Results

The right panel lists the elements out of line first, then the groups,
largest first, with their storeys, and the elements with nothing above or
below. A click selects the element or group and zooms to it.
