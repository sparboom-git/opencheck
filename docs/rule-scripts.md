# Script rules

A script rule is a rule written as a small program, for checks the built-in
rule types (clash, clearance, duplicates, free zone, information) cannot
express. Scripts are written in [Rhai](https://rhai.rs/book/), a small
language that looks like JavaScript or Rust. They are meant to be written with
the help of a language model: give it this page and describe the rule.

A script can only look at the model and report findings. It cannot read or
write files or reach the network, and it is stopped if it runs too long.

## The rule in the ruleset

A script rule is a `[[rule]]` with `type = "script"`. Its numbers go in
`[rule.params]`: they appear as input fields in the app, so people can adjust
the rule without touching the code. Lengths are in millimetres.

```toml
[[rule]]
id = "tray-headroom"
name = "Headroom above cable trays"
type = "script"
severity = "moderate"

[rule.a]                      # the elements the script checks: subjects()
classes = ["IfcCableCarrierSegment"]

[rule.b]                      # what can be in the way: others()
disciplines = ["mechanical", "plumbing", "electrical", "fire_protection"]

[rule.params]
headroom_mm = 200
max_blocked_mm = 1000

script = '''
for tray in subjects() {
    if !tray.run.horizontal { continue; }
    for s in tray.blocked("above", param("headroom_mm")) {
        if s.length > param("max_blocked_mm") {
            report(s, `Headroom blocked over ${s.length} mm`);
        }
    }
}
'''
```

`[rule.a]` and `[rule.b]` are the usual element selectors (disciplines,
classes, names, materials, properties; see the ruleset guide). Without
`[rule.b]`, `others()` is every element of the models the ruleset runs on.

## Units and axes

- Every length is in **millimetres**, as whole numbers (integers).
- Z points up. Positions are in the federation's frame.
- An element's **run** is its longest axis: the direction a cable tray, duct,
  pipe or beam runs in. "Along" means along the run. Sides are seen looking
  along the run; for horizontal runs "above" is up.

## Functions

### Groups and parameters

| Call | Returns |
|---|---|
| `subjects()` | Array of the elements in group a. |
| `others()` | Array of the elements in group b. |
| `param("name")` | The number in `[rule.params]`; an error if it is missing. |
| `print(x)` | Writes a line to the rule's notes in the results (up to 500 lines). |

### An element

| Property or call | Returns |
|---|---|
| `e.name`, `e.global_id` | Name and IFC GlobalId. |
| `e.class` | IFC class as written in the file, e.g. `IfcFlowSegment`. |
| `e.type_class` | The class its type gives it, e.g. `IfcCableCarrierSegment` for Revit's IFC2x3 flow segments. Use this for "what is it". |
| `e.type_name`, `e.object_type` | Type name and ObjectType; empty when not set. |
| `e.systems` | Array of the names of the systems it belongs to. |
| `e.model`, `e.discipline` | Short model name and discipline code (`ARC`, `STR`, `VEN`, `PLU`, `ELE`, `SPR`, ...). |
| `e.storey` | Name of the storey it is placed on; empty if none. |
| `e.storey_elevation` | That storey's elevation in mm, or `()` if unknown. |
| `e.prop("Pset.Name")` | A property value: number, text or true/false; `()` when missing. `e.prop("Name")` searches every property set and the IFC attributes (e.g. `"ObjectType"`). |
| `e.bounds` | Map `min_x, min_y, min_z, max_x, max_y, max_z` of the axis-aligned box. |
| `e.run` | Map `length, width, height, vertical, horizontal, slope`: the element's own dimensions along its run, across it and up; `slope` in degrees. Vertical: within 30° of vertical. Horizontal: within 30° of horizontal. |

### Measuring

| Call | Returns |
|---|---|
| `a.distance(b)` | Shortest distance between the surfaces of two elements in mm; 0 when they touch or cross; at most 10 000. |
| `a.distance(b, max)` | The same, looking only as far as `max` mm (returns `max` when they are farther apart). Much faster; use it when only short distances matter. |
| `e.nearby(mm)` | Array of the elements of `others()` whose boxes come within `mm` of `e`'s box: the candidates worth measuring. |
| `a.connected(b)` | True when `b` is a segment or fitting of the same model touching `a`: the next part of the same run (a bend, a tee, the next duct), not something in its way. |
| `a.joins(b)` | True when `b` continues `a`'s run across a gap of up to 100 mm: they meet end to end, or one ends at the other's side (a branch), as where a coupling or saddle is not modelled. Elements running side by side do not join. |
| `a.angle(b)` | Angle between the runs of `a` and `b` in degrees, 0 (parallel) to 90 (crossing at right angles). |
| `e.blocked(side, depth)` | Array of stretches along `e`'s run where a zone `depth` mm deep on `side` is blocked by `others()`. |
| `e.blocked(side, depth, list)` | The same, blocked by the elements in `list` (an array from `others()` or `subjects()`, filtered as you like). |

Sides for `blocked`:

| Side | Zone |
|---|---|
| `"above"`, `"below"` | Over or under the element, as wide as it is. |
| `"left"`, `"right"` | Beside the element, as high as it is. |
| `"one_side"` | Blocked only where **both** sides are blocked: at least one side must be free. |
| `"front"`, `"back"` | In front of either broad face, for vertical runs (e.g. a vertical cable ladder). |
| `"one_face"` | Blocked only where both faces are blocked. |

Zones stop 50 mm short of the element's ends, so the next segment of the same
run does not count as blocking. Spaces, openings and parts of the same
assembly never block.

A stretch has `s.from`, `s.to` and `s.length` (mm along the run), `s.element`
(the element whose zone it is) and `s.by` (the element blocking most of it,
or `()`).

### Reporting

| Call | Finding |
|---|---|
| `report(e, "message")` | On one element. |
| `report(a, b, "message")` | On two elements, shown in orange and blue. |
| `report(stretch, "message")` | On a blocked stretch: the element and the one blocking it, with the blocked zone drawn in the 3D view. |

Messages can have several lines (`"\n"`). The same finding reported twice is
kept once. The rule's severity applies to all its findings.

## Rhai in brief

```
let x = 5;                    // variables; numbers can be integers or decimals
let text = `tray ${x} mm`;    // text with values inside
if x > 3 && !flag { } else { }
for e in subjects() { }       // loop over an array
for i in 0..10 { }            // 0 to 9
let list = []; list.push(e);  // arrays
let short = list.filter(|e| e.run.length < 1000);
let m = #{ a: 1, b: 2 }; m.a  // maps
if v == () { }                // a missing value
abs(x), min(a, b), max(a, b), x.round(), x.floor()
x % 10                        // remainder
fn helper(e) { e.run.length } // functions (they cannot see outer variables)
```

Lengths from the API are whole millimetres (integers). `param` gives an
integer for whole numbers and a decimal otherwise. Integers and decimals can
be compared with each other; to calculate with both, turn one into the
other with `x.to_float()` or `x.to_int()`.

## Examples

**Service area on at least one side, interruptions up to 1000 mm**

```
for tray in subjects() {
    if !tray.run.horizontal { continue; }
    for s in tray.blocked("one_side", param("service_mm")) {
        if s.length > param("max_blocked_mm") {
            report(s, `No ${param("service_mm")} mm service area on either side over ${s.length} mm`);
        }
    }
}
```

**Headroom with a hard minimum where it is interrupted**

```
for tray in subjects() {
    if !tray.run.horizontal { continue; }
    for s in tray.blocked("above", param("headroom_mm")) {
        if s.length > param("max_blocked_mm") {
            report(s, `Headroom ${param("headroom_mm")} mm blocked over ${s.length} mm`);
        }
    }
    for s in tray.blocked("above", param("min_mm")) {
        report(s, `Less than ${param("min_mm")} mm above the tray`);
    }
}
```

**Distance between vertical ladders, sideways only**

Segments of one ladder touch end to end, so measure beside each ladder
instead of between every pair:

```
let ladders = subjects().filter(|e| e.run.vertical);
for ladder in ladders {
    for side in ["left", "right"] {
        for s in ladder.blocked(side, param("min_mm"), ladders) {
            report(s, `Less than ${param("min_mm")} mm to the next ladder`);
        }
    }
}
```

**Different distances for parallel runs and crossings**

```
for a in subjects() {
    for b in a.nearby(param("parallel_mm")) {
        if a.global_id >= b.global_id || a.connected(b) || a.joins(b) { continue; }
        let need = if a.angle(b) <= param("parallel_angle") {
            param("parallel_mm")
        } else {
            param("crossing_mm")
        };
        let d = a.distance(b, need);
        if d < need {
            report(a, b, `${d} mm apart, at least ${need} mm required`);
        }
    }
}
```

**Distance between two groups**

```
for a in subjects() {
    for b in others() {
        let d = a.distance(b);
        if d < param("min_mm") {
            report(a, b, `Only ${d} mm apart`);
        }
    }
}
```

**Heights on a 10 mm grid above the storey**

```
for tray in subjects() {
    let floor = tray.storey_elevation;
    if floor == () { continue; }
    let bottom = tray.bounds.min_z - floor;
    if bottom % param("grid_mm") != 0 {
        report(tray, `Bottom at ${bottom} mm above floor is not on the ${param("grid_mm")} mm grid`);
    }
}
```

**Using a property**

```
for e in subjects() {
    let system = e.prop("System Name");
    if system == () {
        report(e, "No system name");
    }
}
```
