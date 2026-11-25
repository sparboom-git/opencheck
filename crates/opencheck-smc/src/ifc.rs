//! The models of a coordination file as IFC4 files.
//!
//! Each model becomes one IFC4 file: its elements with their class, GlobalId,
//! name, type, tag and property sets; its site, building and storeys with
//! their names and elevations; and the shape of each element as one
//! `IfcTriangulatedFaceSet` in metres, with the colour most of its corners
//! have. An element is put on the storey its chain of coordinate frames
//! leads to, which is how the file says where it is.
//!
//! The file's coordinates are the checker's shared frame, often georeferenced
//! (hundreds of kilometres from the origin). Every element is written
//! relative to one project origin, the first corner found rounded down to
//! 100 m, and placed by one `IfcLocalPlacement` at that origin, so the
//! numbers in the file stay short and precise.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::{Read, Seek, Write as _};

use crate::mesh;
use crate::stream::{self, Data, Elem, V};
use crate::{ConvertedModel, ModelSummary};

const IFC_SCHEMA: &str = "IFC4";

/// The grey the reader gives a shape without a colour of its own, and pure
/// black, which the file uses for the same: neither is written as a colour.
const NO_COLOUR: [[u8; 3]; 2] = [[184, 184, 189], [0, 0, 0]];

/// Reads the stream of a file: the largest entry of the zip, or the file
/// itself when it is the bare stream.
pub fn read<R: Read + Seek>(mut file: R) -> Result<Data, String> {
    let mut magic = [0u8; 2];
    file.read_exact(&mut magic).map_err(|e| e.to_string())?;
    file.rewind().map_err(|e| e.to_string())?;
    let mut reader = if magic == *b"PK" {
        let mut zip =
            zip::ZipArchive::new(file).map_err(|e| format!("not a coordination file: {e}"))?;
        let largest = (0..zip.len())
            .max_by_key(|&k| zip.by_index(k).map_or(0, |f| f.size()))
            .ok_or("the file is empty")?;
        let entry = zip.by_index(largest).map_err(|e| e.to_string())?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        let mut entry = entry;
        entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        stream::Reader::new(std::io::Cursor::new(bytes))
    } else {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        stream::Reader::new(std::io::Cursor::new(bytes))
    };
    // `File` records are kept, so the models know their IFC files.
    reader.keep = vec!["File".into()];
    reader.run()
}

/// Writes every model of the file that has elements, in the file's order.
pub fn write_models(data: &Data, source_stem: &str) -> Vec<ConvertedModel> {
    let mut per_model: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for (k, e) in data.elems.iter().enumerate() {
        per_model
            .entry(e.model as usize)
            .or_default()
            .push(k as u32);
    }
    let origin = project_origin(data, &per_model);
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut models = Vec::new();
    for (model, elems) in &per_model {
        let short_name = data
            .text(data.models[*model].short)
            .unwrap_or("")
            .trim()
            .to_string();
        let mut name = model_file_stem(data, *model).unwrap_or_else(|| short_name.clone());
        if name.is_empty() {
            name = format!("Model {model}");
        }
        let n = used.entry(name.clone()).or_default();
        *n += 1;
        if *n > 1 {
            name = format!("{name} ({n})");
        }
        let (ifc, summary) = write_model(data, *model, &name, source_stem, elems, origin);
        if summary.elements > 0 {
            models.push(ConvertedModel {
                name,
                short_name,
                ifc,
                summary,
            });
        }
    }
    models
}

/// The name of the file a model came from, without folder and extension.
fn model_file_stem(data: &Data, k: usize) -> Option<String> {
    let V::Rec(r) = data.models[k].file else {
        return None;
    };
    let rec = &data.recs[r as usize];
    let path = data.text(data.field(rec, "File", "path"))?;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    (!stem.is_empty()).then(|| stem.to_string())
}

/// The origin everything is written relative to: the first corner of the
/// first shape in the file, rounded down to 100 m; z stays 0.
fn project_origin(data: &Data, per_model: &BTreeMap<usize, Vec<u32>>) -> [f64; 3] {
    let source = mesh::sources(data);
    let mut shapes = mesh::Shapes::new(data, &source);
    let mut mesh = mesh::Mesh::default();
    for elems in per_model.values() {
        for &k in elems {
            mesh.clear();
            shapes.element(&data.elems[k as usize], &mut mesh);
            if let Some(p) = mesh.positions.as_chunks::<3>().0.first() {
                return [
                    (p[0] / 100.0).floor() * 100.0,
                    (p[1] / 100.0).floor() * 100.0,
                    0.0,
                ];
            }
        }
    }
    [0.0; 3]
}

/// A STEP string: quotes doubled, non-ASCII as `\X2\..\X0\`.
fn step_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    let mut in_x2 = false;
    for c in s.chars() {
        let code = c as u32;
        if code < 128 {
            if in_x2 {
                out.push_str("\\X0\\");
                in_x2 = false;
            }
            match c {
                '\'' => out.push_str("''"),
                '\\' => out.push_str("\\\\"),
                '\n' | '\r' | '\t' => out.push(' '),
                _ if code < 32 => {}
                c => out.push(c),
            }
        } else {
            if !in_x2 {
                out.push_str("\\X2\\");
                in_x2 = true;
            }
            // \X2\ holds UCS-2; a character beyond it is written as '?'
            let _ = write!(out, "{:04X}", if code <= 0xFFFF { code } else { 0x3F });
        }
    }
    if in_x2 {
        out.push_str("\\X0\\");
    }
    out.push('\'');
    out
}

fn opt_str(s: Option<&str>) -> String {
    match s {
        Some(s) if !s.is_empty() => step_str(s),
        _ => "$".into(),
    }
}

/// A STEP real: four decimals, trailing zeros dropped, the point kept.
fn real(x: f64) -> String {
    if !x.is_finite() {
        return "0.".into();
    }
    let mut s = format!("{x:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s == "-0." {
        s = "0.".into();
    }
    s
}

/// IFC GlobalIds for the entities the file has none for (relations,
/// property sets, made-up storeys): from a counter, so they are unique in
/// the file and the same on every run.
struct Guids {
    state: u64,
}

impl Guids {
    fn mix(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next(&mut self) -> String {
        const ALPHABET: &[u8; 64] =
            b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
        let bits: u128 = ((self.mix() as u128) << 64) | self.mix() as u128;
        let mut s = String::with_capacity(22);
        s.push(ALPHABET[((bits >> 126) & 3) as usize] as char);
        for i in 0..21 {
            s.push(ALPHABET[((bits >> (120 - 6 * i)) & 63) as usize] as char);
        }
        s
    }
}

/// The STEP file being written: one entity per line, numbered from 1.
struct Ifc {
    out: Vec<u8>,
    next: u32,
}

impl Ifc {
    fn add(&mut self, entity: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        let _ = writeln!(self.out, "#{id}={entity};");
        id
    }
}

fn refs(ids: &[u32]) -> String {
    let mut s = String::from("(");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "#{id}");
    }
    s.push(')');
    s
}

fn is_a(class: &str, parent: &str) -> bool {
    ifc_lite_core::is_subtype_of_for_schema(IFC_SCHEMA, class, parent)
}

/// The IFC4 class an element is written as: its own when IFC4 has it as a
/// concrete entity, else a proxy.
fn ifc4_class(t: &str) -> (String, bool) {
    if let Some(info) = ifc_lite_core::entity_info_for_schema(IFC_SCHEMA, t)
        && !info.is_abstract
    {
        return (t.to_string(), true);
    }
    ("IfcBuildingElementProxy".to_string(), false)
}

/// The spatial kind of a class: 1 site, 2 building, 3 storey, 4 another
/// spatial element, 0 none.
fn spatial_kind(class: &str) -> u8 {
    match class {
        "IfcSite" => 1,
        "IfcBuilding" => 2,
        "IfcBuildingStorey" => 3,
        c if is_a(c, "IfcSpatialStructureElement") || is_a(c, "IfcSpatialZone") => 4,
        _ => 0,
    }
}

/// The site, building and storey that everything unplaced hangs on: the
/// model's first ones, or made-up ones.
struct Spatial {
    project: u32,
    placement: u32,
    site: Option<u32>,
    building: Option<u32>,
    storey: Option<u32>,
    made_up: usize,
}

impl Spatial {
    fn site(&mut self, ifc: &mut Ifc, guids: &mut Guids, under: &mut Aggregates) -> u32 {
        if let Some(s) = self.site {
            return s;
        }
        self.made_up += 1;
        let s = ifc.add(&format!(
            "IFCSITE({},$,'Site',$,$,#{},$,$,.ELEMENT.,$,$,$,$,$)",
            step_str(&guids.next()),
            self.placement
        ));
        under.entry(self.project).or_default().push(s);
        self.site = Some(s);
        s
    }

    fn building(&mut self, ifc: &mut Ifc, guids: &mut Guids, under: &mut Aggregates) -> u32 {
        if let Some(b) = self.building {
            return b;
        }
        let s = self.site(ifc, guids, under);
        self.made_up += 1;
        let b = ifc.add(&format!(
            "IFCBUILDING({},$,'Building',$,$,#{},$,$,.ELEMENT.,$,$,$)",
            step_str(&guids.next()),
            self.placement
        ));
        under.entry(s).or_default().push(b);
        self.building = Some(b);
        b
    }

    fn storey(&mut self, ifc: &mut Ifc, guids: &mut Guids, under: &mut Aggregates) -> u32 {
        if let Some(st) = self.storey {
            return st;
        }
        let b = self.building(ifc, guids, under);
        self.made_up += 1;
        let st = ifc.add(&format!(
            "IFCBUILDINGSTOREY({},$,'Storey',$,$,#{},$,$,.ELEMENT.,$)",
            step_str(&guids.next()),
            self.placement
        ));
        under.entry(b).or_default().push(st);
        self.storey = Some(st);
        st
    }
}

/// Parent entity -> the entities aggregated under it.
type Aggregates = BTreeMap<u32, Vec<u32>>;

/// Writes one model. Returns the IFC and what was written.
fn write_model(
    data: &Data,
    model: usize,
    name: &str,
    source_stem: &str,
    elems: &[u32],
    origin: [f64; 3],
) -> (Vec<u8>, ModelSummary) {
    let mut guids = Guids {
        state: 0x5EED_0000 + model as u64 * 0x1_0000_0000,
    };
    let mut ifc = Ifc {
        out: Vec::with_capacity(16 << 20),
        next: 1,
    };
    let mut summary = ModelSummary::default();
    let _ = write!(
        ifc.out,
        "ISO-10303-21;\nHEADER;\n\
         FILE_DESCRIPTION(('ViewDefinition [CoordinationView]','Converted by OpenCheck from a coordination file (.smc); shapes as tessellation'),'2;1');\n\
         FILE_NAME({},'',('OpenCheck'),(''),'opencheck-smc {}','OpenCheck','');\n\
         FILE_SCHEMA(('{IFC_SCHEMA}'));\nENDSEC;\nDATA;\n",
        step_str(&format!("{name}.ifc")),
        env!("CARGO_PKG_VERSION")
    );
    // the world frame and the context
    let pt0 = ifc.add("IFCCARTESIANPOINT((0.,0.,0.))");
    let dz = ifc.add("IFCDIRECTION((0.,0.,1.))");
    let dx = ifc.add("IFCDIRECTION((1.,0.,0.))");
    let axis = ifc.add(&format!("IFCAXIS2PLACEMENT3D(#{pt0},#{dz},#{dx})"));
    let ctx = ifc.add(&format!(
        "IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#{axis},$)"
    ));
    let body_ctx = ifc.add(&format!(
        "IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{ctx},$,.MODEL_VIEW.,$)"
    ));
    let u_len = ifc.add("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)");
    let u_area = ifc.add("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)");
    let u_vol = ifc.add("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)");
    let u_ang = ifc.add("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)");
    let units = ifc.add(&format!(
        "IFCUNITASSIGNMENT((#{u_len},#{u_area},#{u_vol},#{u_ang}))"
    ));
    // every element stands in one placement at the project origin, with its
    // coordinates relative to it
    let pt_origin = ifc.add(&format!(
        "IFCCARTESIANPOINT(({},{},{}))",
        real(origin[0]),
        real(origin[1]),
        real(origin[2])
    ));
    let axis_origin = ifc.add(&format!("IFCAXIS2PLACEMENT3D(#{pt_origin},#{dz},#{dx})"));
    let placement = ifc.add(&format!("IFCLOCALPLACEMENT($,#{axis_origin})"));

    let text = |v: V| data.text(v).filter(|s| !s.is_empty());
    let ifc_type = |e: &Elem| text(e.ifc_type).unwrap_or("IfcBuildingElementProxy");
    // a field of the element's own classes: a storey's name and elevation live there
    let own = |e: &Elem, field: &str| -> V {
        e.own
            .iter()
            .find(|(class, f, _)| {
                data.classes[*class as usize]
                    .fields
                    .get(*f as usize)
                    .is_some_and(|x| &*x.name == field)
            })
            .map(|(_, _, v)| *v)
            .unwrap_or(V::Null)
    };
    let own_text = |e: &Elem, field: &str| text(own(e, field));
    let own_metres = |e: &Elem, field: &str| -> Option<f64> {
        if let V::Num(x) = own(e, field) {
            x.is_finite().then(|| f64::from(x) * 0.001)
        } else {
            None
        }
    };

    let project_elem = elems
        .iter()
        .copied()
        .find(|&k| ifc_type(&data.elems[k as usize]) == "IfcProject");
    let project_name = project_elem
        .and_then(|k| text(data.elems[k as usize].name))
        .unwrap_or(source_stem);
    let project_guid = project_elem
        .and_then(|k| text(data.elems[k as usize].guid).filter(|g| g.len() == 22))
        .map(str::to_string)
        .unwrap_or_else(|| guids.next());
    let project = ifc.add(&format!(
        "IFCPROJECT({},$,{},$,$,$,$,(#{ctx}),#{units})",
        step_str(&project_guid),
        step_str(project_name)
    ));

    // which spatial element owns which coordinate frame: an element's frame
    // stands in its storey's, so the chain of frames says where it is
    let mut owner_of_frame: HashMap<u32, u32> = HashMap::new();
    for &k in elems {
        let e = &data.elems[k as usize];
        if spatial_kind(ifc_type(e)) != 0
            && let V::Csys(c) = e.csys
        {
            owner_of_frame.insert(c, k);
        }
    }
    let ancestor_by_frame = |k: u32| -> Option<u32> {
        let mut at = data.elems[k as usize].csys;
        for _ in 0..64 {
            let V::Csys(c) = at else { return None };
            let origin = data.csys[c as usize].1;
            if let V::Csys(o) = origin
                && let Some(owner) = owner_of_frame.get(&o)
                && *owner != k
            {
                return Some(*owner);
            }
            at = origin;
        }
        None
    };

    let source = mesh::sources(data);
    let mut shapes = mesh::Shapes::new(data, &source);
    let mut mesh = mesh::Mesh::default();
    let mut proxies: BTreeMap<String, usize> = BTreeMap::new();
    let mut ifc_id: HashMap<u32, u32> = HashMap::new();
    let mut is_product: HashMap<u32, bool> = HashMap::new();
    let mut kind_of_elem: HashMap<u32, u8> = HashMap::new();
    let mut vertex_of: HashMap<[i64; 3], u32> = HashMap::new();
    let mut style_of: HashMap<[u8; 4], u32> = HashMap::new();
    let mut colour_count: HashMap<[u8; 4], usize> = HashMap::new();

    for &k in elems {
        let e = &data.elems[k as usize];
        let t = ifc_type(e);
        // openings are not drawn or checked; their hosts' shapes are cut already
        if t == "IfcProject" || t.eq_ignore_ascii_case("IfcOpeningElement") {
            continue;
        }
        let (class, own_class) = ifc4_class(t);
        if !own_class {
            *proxies.entry(t.to_string()).or_default() += 1;
        }
        let Some(names) = ifc_lite_core::attribute_names_for_schema(IFC_SCHEMA, &class) else {
            continue;
        };
        let product = is_a(&class, "IfcProduct");
        let mut shape: Option<u32> = None;
        if product {
            mesh.clear();
            shapes.element(e, &mut mesh);
            if mesh.triangles() > 0 {
                // the corners, each once, to 0.1 mm
                vertex_of.clear();
                let mut coords = String::with_capacity(mesh.positions.len() * 8);
                let mut index = String::with_capacity(mesh.indices.len() * 6);
                let mut n_vertices = 0u32;
                let mut remap: Vec<u32> = Vec::with_capacity(mesh.positions.len() / 3);
                for p in mesh.positions.as_chunks::<3>().0 {
                    let key = [
                        ((p[0] - origin[0]) * 10000.0).round() as i64,
                        ((p[1] - origin[1]) * 10000.0).round() as i64,
                        ((p[2] - origin[2]) * 10000.0).round() as i64,
                    ];
                    let id = *vertex_of.entry(key).or_insert_with(|| {
                        n_vertices += 1;
                        if n_vertices > 1 {
                            coords.push(',');
                        }
                        let _ = write!(
                            coords,
                            "({},{},{})",
                            real(key[0] as f64 / 10000.0),
                            real(key[1] as f64 / 10000.0),
                            real(key[2] as f64 / 10000.0)
                        );
                        n_vertices
                    });
                    remap.push(id);
                }
                let mut n_written = 0usize;
                for tri in mesh.indices.as_chunks::<3>().0 {
                    let (a, b, c) = (
                        remap[tri[0] as usize],
                        remap[tri[1] as usize],
                        remap[tri[2] as usize],
                    );
                    if a == b || b == c || a == c {
                        continue;
                    }
                    if n_written > 0 {
                        index.push(',');
                    }
                    let _ = write!(index, "({a},{b},{c})");
                    n_written += 1;
                }
                if n_written > 0 {
                    let list = ifc.add(&format!("IFCCARTESIANPOINTLIST3D(({coords}))"));
                    let faces = ifc.add(&format!(
                        "IFCTRIANGULATEDFACESET(#{list},$,.F.,({index}),$)"
                    ));
                    // the colour most of the element's corners have
                    colour_count.clear();
                    for c in &mesh.colors {
                        *colour_count.entry(*c).or_default() += 1;
                    }
                    let main = colour_count
                        .iter()
                        .max_by_key(|(c, n)| (**n, **c))
                        .map(|(c, _)| *c)
                        .filter(|c| !NO_COLOUR.contains(&[c[0], c[1], c[2]]));
                    if let Some(c) = main {
                        let style = *style_of.entry(c).or_insert_with(|| {
                            let rgb = ifc.add(&format!(
                                "IFCCOLOURRGB($,{},{},{})",
                                real(f64::from(c[0]) / 255.0),
                                real(f64::from(c[1]) / 255.0),
                                real(f64::from(c[2]) / 255.0)
                            ));
                            let transparency = if c[3] == 255 {
                                "$".to_string()
                            } else {
                                real(1.0 - f64::from(c[3]) / 255.0)
                            };
                            let rendering = ifc.add(&format!(
                                "IFCSURFACESTYLERENDERING(#{rgb},{transparency},$,$,$,$,$,$,.FLAT.)"
                            ));
                            ifc.add(&format!("IFCSURFACESTYLE($,.BOTH.,(#{rendering}))"))
                        });
                        ifc.add(&format!("IFCSTYLEDITEM(#{faces},(#{style}),$)"));
                        summary.coloured += 1;
                    }
                    let rep = ifc.add(&format!(
                        "IFCSHAPEREPRESENTATION(#{body_ctx},'Body','Tessellation',(#{faces}))"
                    ));
                    shape = Some(ifc.add(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}))")));
                    summary.triangles += n_written;
                    summary.with_geometry += 1;
                }
            }
        }
        let guid = text(e.guid)
            .filter(|g| g.len() == 22)
            .map(str::to_string)
            .unwrap_or_else(|| guids.next());
        let args: Vec<String> = names
            .iter()
            .map(|name| match *name {
                "GlobalId" => step_str(&guid),
                "Name" => opt_str(text(e.name).or_else(|| own_text(e, "name"))),
                "Description" => {
                    opt_str(text(e.description).or_else(|| own_text(e, "description")))
                }
                "ObjectType" => opt_str(text(e.object_type)),
                "LongName" => opt_str(own_text(e, "longName")),
                "Elevation" | "RefElevation" => {
                    own_metres(e, "bottomElevation").map_or("$".into(), real)
                }
                "ObjectPlacement" => format!("#{placement}"),
                "Representation" => shape.map_or("$".into(), |s| format!("#{s}")),
                "Tag" => opt_str(text(e.tag)),
                "CompositionType" => ".ELEMENT.".into(),
                _ => "$".into(),
            })
            .collect();
        let id = ifc.add(&format!(
            "{}({})",
            class.to_ascii_uppercase(),
            args.join(",")
        ));
        ifc_id.insert(k, id);
        is_product.insert(k, product);
        kind_of_elem.insert(k, spatial_kind(&class));
        summary.elements += 1;
        // the property sets, each property as a single value
        if let V::Props(all) = e.psets {
            for set in data.list(data.props[all as usize].1) {
                let V::Props(ps) = *set else { continue };
                let (set_name, list) = data.props[ps as usize];
                let set_name = text(set_name).unwrap_or("Properties");
                let mut props: Vec<u32> = Vec::new();
                for item in data.list(list) {
                    let V::Named(n) = *item else { continue };
                    let (key, v) = data.named[n as usize];
                    let Some(key) = text(key) else { continue };
                    let value = match v {
                        V::Str(_) => text(v).map(|s| {
                            if s.chars().count() > 255 {
                                format!("IFCTEXT({})", step_str(s))
                            } else {
                                format!("IFCLABEL({})", step_str(s))
                            }
                        }),
                        V::Num(x) if x.is_finite() => {
                            Some(format!("IFCREAL({})", real(f64::from(x))))
                        }
                        V::Int(n) => Some(format!("IFCINTEGER({n})")),
                        V::Long(n) => Some(format!("IFCINTEGER({})", data.longs[n as usize])),
                        V::Bool(b) => {
                            Some(format!("IFCBOOLEAN({})", if b { ".T." } else { ".F." }))
                        }
                        _ => None,
                    };
                    if let Some(value) = value {
                        props.push(ifc.add(&format!(
                            "IFCPROPERTYSINGLEVALUE({},$,{value},$)",
                            step_str(key)
                        )));
                    }
                }
                if props.is_empty() {
                    continue;
                }
                let pset = ifc.add(&format!(
                    "IFCPROPERTYSET({},$,{},$,{})",
                    step_str(&guids.next()),
                    step_str(set_name),
                    refs(&props)
                ));
                ifc.add(&format!(
                    "IFCRELDEFINESBYPROPERTIES({},$,$,$,(#{id}),#{pset})",
                    step_str(&guids.next())
                ));
            }
        }
    }

    // The spatial structure: sites under the project, buildings under sites,
    // storeys under buildings, products under the storey their frames lead
    // to. What the file does not place goes under the first storey.
    let kind_of = |k: u32| -> u8 { kind_of_elem.get(&k).copied().unwrap_or(0) };
    let mut aggregates: Aggregates = BTreeMap::new();
    let mut contained: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut order: Vec<u32> = elems
        .iter()
        .copied()
        .filter(|k| ifc_id.contains_key(k))
        .collect();
    order.sort_by_key(|&k| std::cmp::Reverse(kind_of(k)));
    let first = |kind: u8| {
        order
            .iter()
            .copied()
            .find(|&k| kind_of(k) == kind)
            .map(|k| ifc_id[&k])
    };
    let mut spatial = Spatial {
        project,
        placement,
        site: first(1),
        building: first(2),
        storey: first(3),
        made_up: 0,
    };
    for &k in &order {
        let id = ifc_id[&k];
        let kind = kind_of(k);
        let ancestor = ancestor_by_frame(k).filter(|p| ifc_id.contains_key(p));
        let ancestor_of_kind =
            |want: u8| ancestor.filter(|p| kind_of(*p) == want).map(|p| ifc_id[&p]);
        match kind {
            1 => aggregates.entry(project).or_default().push(id),
            2 => {
                let s = ancestor_of_kind(1)
                    .unwrap_or_else(|| spatial.site(&mut ifc, &mut guids, &mut aggregates));
                aggregates.entry(s).or_default().push(id);
            }
            3 => {
                let b = ancestor_of_kind(2)
                    .unwrap_or_else(|| spatial.building(&mut ifc, &mut guids, &mut aggregates));
                aggregates.entry(b).or_default().push(id);
            }
            4 => {
                let parent = ancestor_of_kind(3)
                    .or_else(|| ancestor_of_kind(2))
                    .unwrap_or_else(|| spatial.storey(&mut ifc, &mut guids, &mut aggregates));
                aggregates.entry(parent).or_default().push(id);
            }
            _ => {
                if !is_product[&k] {
                    continue;
                }
                let storey = match ancestor_of_kind(3).or_else(|| ancestor_of_kind(4)) {
                    Some(p) => {
                        summary.placed += 1;
                        p
                    }
                    None => {
                        summary.unplaced += 1;
                        spatial.storey(&mut ifc, &mut guids, &mut aggregates)
                    }
                };
                contained.entry(storey).or_default().push(id);
            }
        }
    }
    for (parent, children) in &aggregates {
        for chunk in children.chunks(5000) {
            ifc.add(&format!(
                "IFCRELAGGREGATES({},$,$,$,#{parent},{})",
                step_str(&guids.next()),
                refs(chunk)
            ));
        }
    }
    for (storey, products) in &contained {
        for chunk in products.chunks(5000) {
            ifc.add(&format!(
                "IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,{},#{storey})",
                step_str(&guids.next()),
                refs(chunk)
            ));
        }
    }
    let _ = write!(ifc.out, "ENDSEC;\nEND-ISO-10303-21;\n");
    summary.made_up_spatial = spatial.made_up;
    summary.proxies = proxies.into_iter().collect();
    (ifc.out, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_written_as_step() {
        assert_eq!(step_str("Wall"), "'Wall'");
        assert_eq!(step_str("it's"), "'it''s'");
        assert_eq!(step_str("a\\b"), "'a\\\\b'");
        assert_eq!(step_str("Højde"), "'H\\X2\\00F8\\X0\\jde'");
        assert_eq!(step_str("ÆØ"), "'\\X2\\00C600D8\\X0\\'");
        assert_eq!(opt_str(None), "$");
        assert_eq!(opt_str(Some("")), "$");
    }

    #[test]
    fn reals_keep_their_point() {
        assert_eq!(real(2.0), "2.");
        assert_eq!(real(1.5), "1.5");
        assert_eq!(real(0.12345), "0.1235");
        assert_eq!(real(-0.00001), "0.");
        assert_eq!(real(f64::NAN), "0.");
        assert_eq!(real(641900.0), "641900.");
    }

    #[test]
    fn guids_are_22_characters_and_distinct() {
        let mut guids = Guids { state: 1 };
        let a = guids.next();
        let b = guids.next();
        assert_eq!(a.len(), 22);
        assert_ne!(a, b);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'$')
        );
        // the same on every run
        assert_eq!(Guids { state: 1 }.next(), a);
    }

    #[test]
    fn classes_ifc4_lacks_become_proxies() {
        assert_eq!(ifc4_class("IfcWall"), ("IfcWall".to_string(), true));
        assert!(ifc4_class("IfcWallStandardCase").1);
        assert!(ifc4_class("IfcFlowTerminal").1);
        assert_eq!(
            ifc4_class("IfcElectricalElement"),
            ("IfcBuildingElementProxy".to_string(), false)
        );
        assert_eq!(spatial_kind("IfcBuildingStorey"), 3);
        assert_eq!(spatial_kind("IfcSpace"), 4);
        assert_eq!(spatial_kind("IfcWall"), 0);
    }

    /// A stream by hand (as Open BIM Suite's own test): one object of a
    /// class `Tuple3d` with three doubles, which the reader takes for a
    /// point; then the same stream inside a zip.
    #[test]
    fn a_stream_reads_bare_and_zipped() {
        let mut bytes: Vec<u8> = vec![0xac, 0xed, 0, 5, 0x73, 0x72];
        let name = b"javax.vecmath.Tuple3d";
        bytes.extend((name.len() as u16).to_be_bytes());
        bytes.extend(name);
        bytes.extend([0u8; 8]);
        bytes.extend([0x02, 0, 3]);
        for field in [b"x", b"y", b"z"] {
            bytes.extend([b'D', 0, 1]);
            bytes.extend(field);
        }
        bytes.extend([0x78, 0x70]);
        for value in [1.5f64, -2.0, 4000.0] {
            bytes.extend(value.to_be_bytes());
        }
        let data = read(std::io::Cursor::new(bytes.clone())).unwrap();
        assert_eq!(data.points, [[1.5, -2.0, 4000.0]]);
        assert!(data.models.is_empty());
        assert!(write_models(&data, "x").is_empty());

        let mut zipped = Vec::new();
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zipped));
            zip.start_file("x.smc", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&bytes).unwrap();
            zip.finish().unwrap();
        }
        let data = read(std::io::Cursor::new(zipped)).unwrap();
        assert_eq!(data.points, [[1.5, -2.0, 4000.0]]);
    }
}
