//! The objects of a coordination file, built from the models to write.
//!
//! The file is one tree. At its root stands a model without elements, which
//! holds the models; each model holds its objects sorted by class, one node
//! per class; and the relations between the objects of all models (a storey
//! holds elements, an element has a shape) stand in one list at the root.
//! Lengths are millimetres. An element is placed by a chain of frames: its
//! own, its storey's, the building's, the site's and the model's.
//!
//! A type, a material and a system belong to their model; a classification
//! system with its codes belongs to the file, and stands at the root.

use std::collections::{BTreeMap, HashMap};

use super::classes::{self, PropValue, Shape, Table};
use super::java::{ClassId, Graph, Id, Val};
use super::{WriteElement, WriteModel, WriteShape, WriteStorey, WriteSummary, WriteValue};

const OBJECTS: &str = "[Ljava.lang.Object;";
const ITEMS: &str = "[Lcom.~.sae.redox.Item;";
/// The class of a type whose IFC class the checker has none of its own for.
const PLAIN_TYPE: &str = "com.~.sae.~.SType";
const SYSTEM: &str = "com.~.sae.~.construction.buildingservices.SSystem";
const CLASSES: &str = "[Ljava.lang.Class;";
const DISCIPLINES: &str = "[Lcom.~.saf.plugins.modelhandling.BuildingIndustryDomains;";
const CODE_SOURCE: &str = "com.~.saf.plugins.classificationplugin.ClassificationReference$Source";
const FILTER_STATE: &str = "com.~.saf.plugins.filteringplugin.ClassAndPropertyFilter$FilterState";
/// The class of an element whose IFC class the checker has none of its own for.
const PLAIN_OBJECT: &str = "com.~.sae.~.construction.SObject";
const FAVORITES: &str =
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<Favorites name=\"My Favorites\" />\r\n";
const GUID_CHARS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// The stream of the file and what was written.
pub fn stream(models: &[WriteModel], file_name: &str) -> (Vec<u8>, WriteSummary) {
    let mut graph = Graph::default();
    let table = classes::load(&mut graph);
    let lower = table
        .written_as
        .iter()
        .map(|(ifc, class)| (ifc.to_ascii_lowercase(), class.clone()))
        .collect();
    let type_lower = table
        .type_written_as
        .iter()
        .map(|(ifc, class)| (ifc.to_ascii_lowercase(), class.clone()))
        .collect();
    let mut seed = 0xcbf2_9ce4_8422_2325u64;
    for byte in file_name
        .bytes()
        .chain(models.iter().flat_map(|m| m.name.bytes()))
    {
        seed = (seed ^ byte as u64).wrapping_mul(0x0100_0000_01b3);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let zero = {
        let class = graph.class_short("Double").expect("Double");
        graph.object(class)
    };
    let mut b = Builder {
        g: graph,
        table,
        lower,
        type_lower,
        classified: Vec::new(),
        classified_at: HashMap::new(),
        constants: HashMap::new(),
        last_id: 0,
        seed,
        now,
        zero,
        int_zero: None,
        truth: [None, None],
        relations: Vec::new(),
        summary: WriteSummary::default(),
        as_objects: BTreeMap::new(),
    };
    let root = b.root(models, file_name);
    b.summary.models = models.len();
    b.summary.as_objects = b.as_objects.into_iter().collect();
    (b.g.write(root), b.summary)
}

/// The objects of a model by class, in the order the classes came.
#[derive(Default)]
struct Nodes {
    order: Vec<ClassId>,
    of: HashMap<ClassId, Vec<Val>>,
}

impl Nodes {
    fn add(&mut self, class: ClassId, object: Id) {
        if !self.of.contains_key(&class) {
            self.order.push(class);
        }
        self.of.entry(class).or_default().push(object.into());
    }
}

/// What a model keeps while its elements are written.
struct ModelParts {
    model: Id,
    /// The model's own frame.
    frame: Id,
    /// The types, with the elements of each.
    types: Vec<(Id, Vec<Id>)>,
    /// A type by its class and name.
    type_at: HashMap<(String, String), usize>,
    /// The systems, with the elements of each.
    systems: Vec<(Id, Vec<Id>)>,
    system_at: HashMap<String, usize>,
    /// The list of materials for each set of material names.
    materials: HashMap<Vec<String>, Id>,
}

/// A classification system of the file.
struct Classified {
    object: Id,
    name: String,
    codes: Vec<Code>,
    code_at: HashMap<String, usize>,
    /// The classes of the elements that have a code of the system.
    classes: Vec<ClassId>,
}

/// A code of a classification system, with the elements that have it.
struct Code {
    code: String,
    reference: Id,
    elements: Vec<Id>,
}

struct Builder {
    g: Graph,
    table: Table,
    /// IFC class in small letters -> the class it is written as.
    lower: HashMap<String, String>,
    /// IFC type class in small letters -> the class its type is written as.
    type_lower: HashMap<String, String>,
    /// The classification systems of the file, in the order they came.
    classified: Vec<Classified>,
    classified_at: HashMap<String, usize>,
    /// The constants of enumerations made so far.
    constants: HashMap<(ClassId, &'static str), Id>,
    /// The number the last object got; the root remembers it.
    last_id: i32,
    seed: u64,
    /// Now, in milliseconds since 1970.
    now: i64,
    /// The number 0.0, which every quantity not worked out points to.
    zero: Id,
    int_zero: Option<Id>,
    truth: [Option<Id>; 2],
    relations: Vec<Val>,
    summary: WriteSummary,
    as_objects: BTreeMap<String, usize>,
}

impl Builder {
    // ---- small things ----

    fn class(&self, short: &str) -> ClassId {
        self.g
            .class_short(short)
            .unwrap_or_else(|| panic!("the table has no class {short}"))
    }

    /// A class by its full name, written with `~` as in the table.
    fn class_named(&self, name: &str) -> ClassId {
        self.g
            .class_named(&classes::full(name))
            .unwrap_or_else(|| panic!("the table has no class {name}"))
    }

    fn random(&mut self) -> u64 {
        self.seed = self.seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A GlobalId for an object the models have none for.
    fn guid(&mut self) -> String {
        let mut n = ((self.random() as u128) << 64) | self.random() as u128;
        let mut out = [0u8; 22];
        for slot in out.iter_mut().rev() {
            *slot = GUID_CHARS[(n & 63) as usize];
            n >>= 6;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn uuid(&mut self) -> Id {
        // a random one: version 4, variant 1
        let most = (self.random() & !0xf000) | 0x4000;
        let least = (self.random() & !(0b11 << 62)) | (0b10 << 62);
        let class = self.class("UUID");
        let id = self.g.object(class);
        self.g
            .set(id, "UUID", "mostSigBits", Val::Long(most as i64));
        self.g
            .set(id, "UUID", "leastSigBits", Val::Long(least as i64));
        id
    }

    fn text(&mut self, text: &str) -> Id {
        self.g.text(text)
    }

    fn opt_text(&mut self, text: Option<&str>) -> Val {
        match text {
            Some(t) => self.text(t).into(),
            None => Val::Null,
        }
    }

    fn number(&mut self, value: f64) -> Id {
        if value == 0.0 {
            return self.zero;
        }
        let class = self.class("Double");
        let id = self.g.object(class);
        self.g.set(id, "Double", "value", Val::Double(value));
        id
    }

    fn integer(&mut self, value: i32) -> Id {
        if value == 0
            && let Some(id) = self.int_zero
        {
            return id;
        }
        let class = self.class("Integer");
        let id = self.g.object(class);
        self.g.set(id, "Integer", "value", Val::Int(value));
        if value == 0 {
            self.int_zero = Some(id);
        }
        id
    }

    fn boolean(&mut self, value: bool) -> Id {
        if let Some(id) = self.truth[value as usize] {
            return id;
        }
        let class = self.class("Boolean");
        let id = self.g.object(class);
        self.g.set(id, "Boolean", "value", Val::Bool(value));
        self.truth[value as usize] = Some(id);
        id
    }

    fn date(&mut self, millis: i64) -> Id {
        let class = self.class("Date");
        let id = self.g.object(class);
        self.g
            .extra(id, "Date", Val::Block(millis.to_be_bytes().to_vec()));
        id
    }

    fn file(&mut self, path: &str) -> Id {
        let class = self.class("File");
        let id = self.g.object(class);
        let path = self.text(path);
        self.g.set(id, "File", "path", path);
        // the character between folders where the file was written
        self.g.extra(id, "File", Val::Block(vec![0x00, b'\\']));
        id
    }

    fn list(&mut self, items: Vec<Val>) -> Id {
        let class = self.class("ArrayList");
        let id = self.g.object(class);
        let n = items.len() as i32;
        self.g.set(id, "ArrayList", "size", Val::Int(n));
        self.g
            .extra(id, "ArrayList", Val::Block(n.to_be_bytes().to_vec()));
        for item in items {
            self.g.extra(id, "ArrayList", item);
        }
        id
    }

    fn vector(&mut self, mut items: Vec<Val>) -> Id {
        let n = items.len() as i32;
        if items.is_empty() {
            items.push(Val::Null);
        }
        let objects = self.class_named(OBJECTS);
        let data = self.g.objects(objects, items);
        let class = self.class("Vector");
        let id = self.g.object(class);
        self.g.set(id, "Vector", "elementCount", Val::Int(n));
        self.g.set(id, "Vector", "elementData", data);
        id
    }

    fn empty_map(&mut self) -> Id {
        let class = self.class("HashMap");
        let id = self.g.object(class);
        self.g.set(id, "HashMap", "loadFactor", Val::Float(0.75));
        self.g.set(id, "HashMap", "threshold", Val::Int(1));
        // buckets and entries
        self.g
            .extra(id, "HashMap", Val::Block(vec![0, 0, 0, 1, 0, 0, 0, 0]));
        id
    }

    // ---- properties ----

    /// A property object: every value of the checker's objects is one, with
    /// the object it belongs to, a status and its value.
    fn prop(
        &mut self,
        class: &str,
        parent: Id,
        status: i32,
        name: Option<&str>,
        value_type: Option<&str>,
        value: Val,
    ) -> Id {
        let class = self.class(class);
        let id = self.g.object(class);
        self.g.set(id, "Item", "parent", parent);
        self.g.set(id, "Property", "status", Val::Int(status));
        let name = self.opt_text(name);
        self.g.set(id, "Property", "name", name);
        let value_type = self.opt_text(value_type);
        self.g.set(id, "Property", "importedValueType", value_type);
        self.g.set(id, "Property", "value", value);
        id
    }

    fn text_prop(&mut self, parent: Id, status: i32, value: Option<&str>) -> Id {
        let value = self.opt_text(value);
        self.prop("StringProperty", parent, status, None, None, value)
    }

    /// A set of properties with a name, or the nameless one an object keeps
    /// its sets in.
    fn property_set(
        &mut self,
        parent: Id,
        name: Option<&str>,
        source: Option<&str>,
        properties: Option<Vec<Val>>,
    ) -> Id {
        let id = self.prop("PropertySet", parent, 0, name, None, Val::Null);
        if let Some(properties) = properties {
            let list = self.list(properties);
            self.g.set(id, "PropertySet", "propertyList", list);
        }
        let source = self.opt_text(source);
        self.g.set(id, "PropertySet", "source", source);
        id
    }

    /// Fills the fields of an object's own classes with what they hold when
    /// nothing is known: mostly quantities the checker works out itself.
    fn defaults(&mut self, object: Id) {
        let chain = self.g.class(self.g.class_of(object)).chain.clone();
        for class in chain {
            let (name, short) = {
                let of = self.g.class(class);
                (of.name.clone(), of.short().to_string())
            };
            let Some(fields) = self.table.defaults.get(&name).cloned() else {
                continue;
            };
            for (field, shape) in fields {
                let ty = self
                    .g
                    .class(class)
                    .fields
                    .iter()
                    .find(|f| f.name == field)
                    .map_or(b'L', |f| f.ty);
                let value = match shape {
                    Shape::Null => Val::Null,
                    Shape::Bool(v) => Val::Bool(v),
                    Shape::Int(v) if ty == b'J' => Val::Long(v as i64),
                    Shape::Int(v) => Val::Int(v),
                    Shape::Num(v) if ty == b'F' => Val::Float(v as f32),
                    Shape::Num(v) => Val::Double(v),
                    Shape::Prop {
                        class,
                        status,
                        quantity,
                        value,
                    } => {
                        let value = match value {
                            PropValue::Null => Val::Null,
                            PropValue::Text(t) => self.text(&t).into(),
                            PropValue::Double(v) => self.number(v).into(),
                            PropValue::Integer(v) => self.integer(v).into(),
                            PropValue::Boolean(v) => self.boolean(v).into(),
                            PropValue::Object(class, numbers) => {
                                self.plain_object(&class, &numbers).into()
                            }
                        };
                        let prop = self.prop(&class, object, status, None, None, value);
                        if let Some(q) = quantity {
                            self.g
                                .set(prop, "DoubleProperty", "quantityType", Val::Int(q));
                        }
                        prop.into()
                    }
                };
                self.g.set(object, &short, &field, value);
            }
        }
    }

    /// An object of whole numbers only, e.g. the way a door opens.
    fn plain_object(&mut self, class: &str, numbers: &[i32]) -> Id {
        let class = self.class_named(class);
        let id = self.g.object(class);
        let of = self.g.class(class);
        let (short, fields): (String, Vec<String>) = (
            of.short().to_string(),
            of.fields
                .iter()
                .filter(|f| f.ty == b'I')
                .map(|f| f.name.clone())
                .collect(),
        );
        for (field, n) in fields.iter().zip(numbers) {
            self.g.set(id, &short, field, Val::Int(*n));
        }
        id
    }

    // ---- objects with a number ----

    /// An object of a model: it gets the next number and, unless it is a
    /// model itself, an identity of its own.
    fn entity(&mut self, class: ClassId, identity: bool) -> Id {
        let id = self.g.object(class);
        self.last_id += 1;
        self.g.set(id, "Entity", "id", Val::Int(self.last_id));
        if identity {
            let uuid = self.uuid();
            self.g.set(id, "Entity", "uuid", uuid);
        }
        id
    }

    /// A frame: where something stands in the frame it points to. Without a
    /// matrix it stands at that frame's origin.
    fn frame(
        &mut self,
        nodes: &mut Nodes,
        parent: Option<Id>,
        matrix: Option<[f64; 16]>,
        within: Option<Id>,
    ) -> Id {
        let class = self.class("CoordinateSystem");
        let id = self.entity(class, true);
        self.g.set(id, "Item", "parent", parent);
        if let Some(matrix) = matrix {
            let doubles = self.class_named("[D");
            let array = self.g.doubles(doubles, matrix.to_vec());
            self.g.set(id, "CoordinateSystem", "matrix", array);
        }
        let status = if within.is_some() { 4 } else { 0 };
        let origin = self.prop("ObjectProperty", id, status, None, None, within.into());
        self.g.set(id, "CoordinateSystem", "origin", origin);
        nodes.add(class, id);
        id
    }

    /// An object of the building model: a project, a site, a storey, a wall.
    #[allow(clippy::too_many_arguments)]
    fn building_object(
        &mut self,
        nodes: &mut Nodes,
        class: &str,
        ifc_class: &str,
        guid: &str,
        label: Option<&str>,
        object_type: Option<&str>,
        predefined_type: Option<&str>,
        description: Option<&str>,
    ) -> Id {
        let class = self.class_named(class);
        let id = self.entity(class, true);
        let value = self.text_prop(id, 4, description);
        self.g.set(id, "SEntity", "description", value);
        let value = self.text(ifc_class);
        self.g.set(id, "SEntity", "ifcType", value);
        let value = self.text_prop(id, 4, label);
        self.g.set(id, "SEntity", "label", value);
        let value = self.text_prop(id, 4, object_type);
        self.g.set(id, "SEntity", "objectType", value);
        let status = if predefined_type.is_some() { 1 } else { 0 };
        let value = self.text_prop(id, status, predefined_type);
        self.g.set(id, "SEntity", "predefinedType", value);
        let value = self.text(guid);
        self.g.set(id, "SEntity", "uniqueId", value);
        self.defaults(id);
        nodes.add(class, id);
        id
    }

    fn relation(&mut self, class: &str, from: Id, name: Option<&str>, to: Vec<Id>) {
        let items = self.class_named(ITEMS);
        let members = self
            .g
            .objects(items, to.into_iter().map(Val::Ref).collect());
        let relation = self.prop(class, from, 4, name, None, members.into());
        self.relations.push(relation.into());
    }

    fn holds(&mut self, from: Id, to: Vec<Id>) {
        if !to.is_empty() {
            let name = self.guid();
            self.relation("SContains", from, Some(&name), to);
        }
    }

    // ---- the file ----

    fn class_nodes(&mut self, model: Id, nodes: Nodes) -> Id {
        let node_class = self.class("Model$ClassNode");
        // the shapes come last, as in the checker's own files: an element is
        // then written before its shape is met on its own
        let shapes = self.class("BoundaryRepresentation");
        let mut order = nodes.order;
        if let Some(at) = order.iter().position(|c| *c == shapes) {
            let class = order.remove(at);
            order.push(class);
        }
        let mut out = Vec::new();
        for class in order {
            let node = self.g.object(node_class);
            let class_object = self.g.class_object(class);
            self.g.set(node, "Model$ClassNode", "clazz", class_object);
            let name = format!("class {}", self.g.class(class).short());
            let objects = self.vector(nodes.of[&class].clone());
            let entities = self.prop(
                "ObjectProperty",
                model,
                5,
                Some(&name),
                None,
                objects.into(),
            );
            self.g.set(node, "Model$ClassNode", "entities", entities);
            out.push(node.into());
        }
        self.vector(out)
    }

    /// The fields every model has, the root too.
    fn model_fields(&mut self, model: Id, file: &str) {
        self.g.set(model, "Model", "savable", Val::Bool(true));
        self.g.set(model, "Model", "versionOnStream", Val::Int(2));
        let map = self.empty_map();
        self.g.set(model, "Model", "externalFiles", map);
        let file = self.file(file);
        self.g.set(model, "Model", "file", file);
        let uuid = self.uuid();
        self.g.set(model, "Model", "modelUuid", uuid);
        let version = {
            let class = self.class("Version");
            let id = self.g.object(class);
            self.g.set(id, "Version", "versionNumber", Val::Int(1));
            let date = self.date(self.now);
            self.g.set(id, "Version", "date", date);
            id
        };
        self.g.set(model, "Model", "version", version);
        let list = self.list(Vec::new());
        self.g.set(model, "SModel", "customHierarchies", list);
        let favorites = {
            let class = self.class("CollSer");
            let id = self.g.object(class);
            // a list of one
            self.g.set(id, "CollSer", "tag", Val::Int(4));
            self.g.extra(id, "CollSer", Val::Block(vec![0, 0, 0, 1]));
            let text = self.text(FAVORITES);
            self.g.extra(id, "CollSer", text);
            id
        };
        self.g.set(model, "SModel", "infoFavorites", favorites);
    }

    fn root(&mut self, models: &[WriteModel], file_name: &str) -> Id {
        let class = self.class("SModel");
        let root = self.g.object(class);
        let additional = self.property_set(root, Some("Additional Data"), None, None);
        let sets = self.property_set(root, None, None, Some(vec![additional.into()]));
        self.g.set(root, "Entity", "extensionPropertySupport", sets);

        let mut nodes = Nodes::default();
        for model in models {
            let id = self.model(model);
            nodes.add(class, id);
        }
        self.classifications(&mut nodes);
        let classes = self.class_nodes(root, nodes);
        self.g.set(root, "Model", "classes", classes);
        self.model_fields(root, file_name);
        self.g
            .set(root, "SModel", "updateStrategyOrdinal", Val::Int(-1));
        let score = self.uuid();
        self.g.set(root, "SModel", "scoreId", score);
        let volume = self.prop("DoubleProperty", root, 2, None, None, Val::Null);
        self.g
            .set(volume, "DoubleProperty", "quantityType", Val::Int(3));
        self.g.set(root, "SModel", "volume", volume);

        let relations = std::mem::take(&mut self.relations);
        let relations = self.list(relations);
        self.g.extra(root, "Model", relations);
        self.g.extra(root, "Model", Val::Null);
        self.g.set(root, "Model", "lastId", Val::Int(self.last_id));
        root
    }

    fn model(&mut self, m: &WriteModel) -> Id {
        let class = self.class("SModel");
        let model = self.entity(class, false);
        let mut nodes = Nodes::default();

        let model_frame = self.frame(&mut nodes, Some(model), None, None);
        // the model's own frame is listed last, after those that stand in it
        let frames = self.class("CoordinateSystem");
        nodes.of.get_mut(&frames).expect("a frame").clear();
        let children = self.vector(vec![model_frame.into()]);
        self.g.set(model, "Entity", "children", children);
        self.g.set(model, "SModel", "coordinateSystem", model_frame);

        // what the model says of itself
        let categories = self.prop(
            "ObjectProperty",
            model,
            0,
            Some("Categories"),
            None,
            Val::Null,
        );
        let additional = self.property_set(
            model,
            Some("Additional Data"),
            None,
            Some(vec![categories.into()]),
        );
        let file_name = format!("{}.ifc", m.name);
        let value = self.text(&file_name);
        let name = self.prop("StringProperty", model, 1, Some("NAME"), None, value.into());
        let ifc_file = self.property_set(
            model,
            Some("IFC_FILE_NAME"),
            Some("IFC"),
            Some(vec![name.into()]),
        );
        let sets = self.property_set(
            model,
            None,
            None,
            Some(vec![additional.into(), ifc_file.into()]),
        );
        self.g
            .set(model, "Entity", "extensionPropertySupport", sets);

        self.owner_history(&mut nodes, m);

        // project, site, building and storeys, each in the frame of the one above
        let guid = self.guid();
        let project = self.building_object(
            &mut nodes,
            "com.~.sae.~.construction.SProject",
            "IfcProject",
            &guid,
            Some(if m.project.is_empty() {
                &m.name
            } else {
                &m.project
            }),
            None,
            None,
            None,
        );
        let project_frame = self.frame(&mut nodes, Some(project), None, Some(model_frame));
        let children = self.vector(vec![project_frame.into()]);
        self.g.set(project, "Entity", "children", children);
        self.g
            .set(project, "SEntity", "coordinateSystem", project_frame);
        let units = self.units(project);
        let sets = self.property_set(project, None, None, Some(vec![units.into()]));
        self.g
            .set(project, "Entity", "extensionPropertySupport", sets);

        let guid = self.guid();
        let site = self.building_object(
            &mut nodes,
            "com.~.sae.~.construction.SSite",
            "IfcSite",
            &guid,
            Some("Site"),
            None,
            None,
            None,
        );
        let site_frame = self.frame(&mut nodes, None, None, Some(model_frame));
        self.g.set(site, "SEntity", "coordinateSystem", site_frame);

        let guid = self.guid();
        let building = self.building_object(
            &mut nodes,
            "com.~.sae.~.construction.SBuilding",
            "IfcBuilding",
            &guid,
            Some("Building"),
            None,
            None,
            None,
        );
        let building_frame = self.frame(&mut nodes, None, None, Some(site_frame));
        self.g
            .set(building, "SEntity", "coordinateSystem", building_frame);

        // a model without storeys gets one, so its elements have a place
        let made_up = [WriteStorey {
            guid: self.guid(),
            name: "Level".into(),
            elevation: 0.0,
        }];
        let storeys: &[WriteStorey] = if m.storeys.is_empty() {
            &made_up
        } else {
            &m.storeys
        };
        let mut storey_objects = Vec::new();
        for storey in storeys {
            let made_up_guid = storey.guid.is_empty().then(|| self.guid());
            let id = self.building_object(
                &mut nodes,
                "com.~.sae.~.construction.SBuildingStorey",
                "IfcBuildingStorey",
                made_up_guid.as_deref().unwrap_or(&storey.guid),
                Some(&storey.name),
                None,
                None,
                None,
            );
            let millimetres = storey.elevation * 1000.0;
            let matrix = (millimetres != 0.0).then(|| translation([0.0, 0.0, millimetres]));
            let frame = self.frame(&mut nodes, None, matrix, Some(building_frame));
            self.g.set(id, "SEntity", "coordinateSystem", frame);
            for field in ["name", "longName"] {
                let value = self.text_prop(id, 4, Some(&storey.name));
                self.g.set(id, "SBuildingStorey", field, value);
            }
            let value = self.number(millimetres);
            let bottom = self.prop("DoubleProperty", id, 4, None, None, value.into());
            self.g
                .set(bottom, "DoubleProperty", "quantityType", Val::Int(1));
            self.g.set(id, "SProduct", "bottomElevation", bottom);
            storey_objects.push((id, frame, storey.elevation, Vec::new()));
        }
        self.holds(project, vec![site]);
        self.holds(site, vec![building]);
        self.holds(building, storey_objects.iter().map(|s| s.0).collect());

        let mut parts = ModelParts {
            model,
            frame: model_frame,
            types: Vec::new(),
            type_at: HashMap::new(),
            systems: Vec::new(),
            system_at: HashMap::new(),
            materials: HashMap::new(),
        };
        for element in &m.elements {
            let at = element
                .storey
                .filter(|k| *k < storey_objects.len())
                .unwrap_or(0);
            let (_, frame, elevation, _) = storey_objects[at];
            let id = self.element(&mut nodes, &mut parts, element, frame, elevation);
            storey_objects[at].3.push(id);
        }
        for (kind, elements) in parts.types {
            let name = self.guid();
            self.relation("SDefinesByType", kind, Some(&name), elements);
        }
        for (system, elements) in parts.systems {
            let name = self.guid();
            self.relation("Groups", system, Some(&name), elements);
        }
        for (storey, _, _, elements) in storey_objects {
            self.holds(storey, elements);
        }
        nodes.add(frames, model_frame);

        let classes = self.class_nodes(model, nodes);
        self.g.set(model, "Model", "classes", classes);
        self.model_fields(model, &format!("{}.smc", m.name));
        let short = self.text(&m.short_name);
        self.g.set(model, "Model", "shortName", short);
        self.g.extra(model, "Model", Val::Null);
        self.g.extra(model, "Model", Val::Null);
        self.g
            .set(model, "SModel", "updateStrategyOrdinal", Val::Int(3));
        let application = self.text(&m.application);
        self.g.set(model, "SModel", "applicationName", application);
        let domain = {
            let class = self.class("BuildingIndustryDomains");
            let id = self.g.object(class);
            let index = m.discipline.number();
            self.g
                .set(id, "BuildingIndustryDomains", "index", Val::Int(index));
            id
        };
        self.g.set(model, "SModel", "domain", domain);
        let zero = self.zero;
        let volume = self.prop("DoubleProperty", model, 1, None, None, zero.into());
        self.g
            .set(volume, "DoubleProperty", "quantityType", Val::Int(3));
        self.g.set(model, "SModel", "volume", volume);
        model
    }

    /// The units of the model, as the checker notes them from an IFC file.
    fn units(&mut self, project: Id) -> Id {
        let mut units = Vec::new();
        for (kind, unit) in [
            ("AREAUNIT", "SQUARE_METRE"),
            ("LENGTHUNIT", "MILLI METRE"),
            ("VOLUMEUNIT", "CUBIC_METRE"),
            ("PLANEANGLEUNIT", "RADIAN"),
        ] {
            let value = self.text(unit);
            let prop = self.prop(
                "StringProperty",
                project,
                4,
                Some(kind),
                Some("IFCSIUNIT"),
                value.into(),
            );
            units.push(prop.into());
        }
        self.property_set(project, Some("IfcUnitAssignment"), Some("IFC"), Some(units))
    }

    /// Who made the model and with what: the checker keeps one per model.
    fn owner_history(&mut self, nodes: &mut Nodes, m: &WriteModel) {
        let class = self.class("SOwnerHistory");
        let history = self.entity(class, true);
        nodes.add(class, history);
        let application = if m.application.is_empty() {
            "OpenCheck"
        } else {
            &m.application
        };
        let value = self.text(application);
        self.g.set(history, "SOwnerHistory", "applicationId", value);

        let organization_class = self.class("SOrganization");
        let organization = |b: &mut Builder, nodes: &mut Nodes| {
            let id = b.entity(organization_class, true);
            for field in ["description", "id", "name"] {
                let value = b.text_prop(id, 4, None);
                b.g.set(id, "SOrganization", field, value);
            }
            nodes.add(organization_class, id);
            id
        };
        let made_by = organization(self, nodes);
        let owned_by = organization(self, nodes);
        let person_class = self.class("SPerson");
        let person = self.entity(person_class, true);
        for field in ["familyName", "givenName", "id"] {
            let value = self.text_prop(person, 4, None);
            self.g.set(person, "SPerson", field, value);
        }
        nodes.add(person_class, person);

        let strings = self.class_named("[Ljava.lang.String;");
        let roles = self.g.objects(strings, Vec::new());
        let created = self.date(self.now);
        let version = self.text("");
        let identifier = self.text("OpenCheck");
        let props = vec![
            self.prop(
                "ObjectProperty",
                history,
                4,
                Some("applicationOrganization"),
                None,
                made_by.into(),
            ),
            self.prop(
                "StringProperty",
                history,
                4,
                Some("applicationVersion"),
                None,
                version.into(),
            ),
            self.prop(
                "DateProperty",
                history,
                4,
                Some("creationDate"),
                None,
                created.into(),
            ),
            self.prop(
                "DateProperty",
                history,
                4,
                Some("lastModifiedDate"),
                None,
                Val::Null,
            ),
            self.prop(
                "ObjectProperty",
                history,
                4,
                Some("organization"),
                None,
                owned_by.into(),
            ),
            self.prop(
                "ObjectProperty",
                history,
                4,
                Some("user"),
                None,
                person.into(),
            ),
            self.prop(
                "ObjectProperty",
                history,
                4,
                Some("userRoles"),
                None,
                roles.into(),
            ),
            self.prop(
                "StringProperty",
                history,
                4,
                Some("applicationIdentifier"),
                None,
                identifier.into(),
            ),
        ];
        let sets = self.property_set(
            history,
            None,
            None,
            Some(props.into_iter().map(Val::Ref).collect()),
        );
        self.g
            .set(history, "Entity", "extensionPropertySupport", sets);
    }

    fn element(
        &mut self,
        nodes: &mut Nodes,
        parts: &mut ModelParts,
        e: &WriteElement,
        storey_frame: Id,
        storey_elevation: f64,
    ) -> Id {
        let model = parts.model;
        let class = match self.lower.get(&e.ifc_class.to_ascii_lowercase()) {
            Some(class) => class.clone(),
            None => {
                *self.as_objects.entry(e.ifc_class.clone()).or_default() += 1;
                PLAIN_OBJECT.to_string()
            }
        };
        let id = self.building_object(
            nodes,
            &class,
            &e.ifc_class,
            &e.guid,
            Some(&e.name).filter(|n| !n.is_empty()).map(String::as_str),
            e.object_type.as_deref(),
            e.predefined_type.as_deref(),
            e.description.as_deref(),
        );
        self.summary.elements += 1;
        let tag = self.opt_text(e.tag.as_deref());
        self.g.set(id, "SProduct", "bimToolId", tag);

        // the element's frame stands at the lowest corner of its shape, so the
        // numbers of the shape stay small and keep their precision
        let shape = e.shape.as_ref().filter(|s| !s.triangles.is_empty());
        let corner = shape.map(|s| {
            let mut low = [f64::MAX; 3];
            for p in &s.positions {
                for k in 0..3 {
                    low[k] = low[k].min(p[k]);
                }
            }
            low
        });
        let matrix = corner.map(|c| {
            translation([
                c[0] * 1000.0,
                c[1] * 1000.0,
                (c[2] - storey_elevation) * 1000.0,
            ])
        });
        let frame = self.frame(nodes, None, matrix, Some(storey_frame));
        self.g.set(id, "SEntity", "coordinateSystem", frame);
        if let (Some(shape), Some(corner)) = (shape, corner) {
            let faces = self.faces(nodes, id, frame, shape, corner);
            let children = self.vector(vec![faces.into()]);
            self.g.set(id, "Entity", "children", children);
            self.relation("SHasGeometry", id, None, vec![faces]);
            self.summary.with_shape += 1;
            self.summary.triangles += shape.triangles.len();
        }

        if !e.property_sets.is_empty() {
            let mut sets = Vec::new();
            for set in &e.property_sets {
                let mut props = Vec::new();
                for p in &set.properties {
                    let value_type = p.value_type.as_deref().map(str::to_ascii_uppercase);
                    let value_type = value_type.as_deref();
                    let name = Some(p.name.as_str());
                    let prop = match &p.value {
                        WriteValue::Text(t) => {
                            let value = self.text(t);
                            self.prop("StringProperty", model, 4, name, value_type, value.into())
                        }
                        WriteValue::Number(n) => {
                            // a plain number: no unit is claimed for it
                            let value = self.number(*n);
                            self.prop("DoubleProperty", model, 4, name, value_type, value.into())
                        }
                        WriteValue::Integer(n) => {
                            let value =
                                self.integer((*n).clamp(i32::MIN as i64, i32::MAX as i64) as i32);
                            self.prop("IntegerProperty", model, 4, name, value_type, value.into())
                        }
                        WriteValue::Bool(v) => {
                            let value = self.boolean(*v);
                            self.prop("BooleanProperty", model, 4, name, value_type, value.into())
                        }
                    };
                    props.push(prop.into());
                }
                let set = self.property_set(model, Some(&set.name), Some("IFC"), Some(props));
                sets.push(set.into());
            }
            let all = self.property_set(id, None, None, Some(sets));
            self.g.set(id, "Entity", "extensionPropertySupport", all);
        }
        self.more(nodes, parts, e, id);
        id
    }

    /// The shape of an element as faces of three points, in millimetres from
    /// the element's own frame.
    ///
    /// The checker's own files hold every shape a second time, as the
    /// triangles it draws with their normals and colours. That copy is left
    /// empty here: the checker works it out from the faces when it opens the
    /// file, and without it a file is less than half the size.
    fn faces(
        &mut self,
        nodes: &mut Nodes,
        element: Id,
        frame: Id,
        shape: &WriteShape,
        corner: [f64; 3],
    ) -> Id {
        let mut points = Vec::with_capacity(shape.triangles.len() * 9);
        let mut faces = 0;
        // a colour per corner, when the shape has more than one
        let several = shape.triangle_colors.len() == shape.triangles.len()
            && shape.triangle_colors.windows(2).any(|w| w[0] != w[1]);
        let mut colours = Vec::new();
        for (t, triangle) in shape.triangles.iter().enumerate() {
            let mut p = [[0.0f64; 3]; 3];
            let mut whole = true;
            for (k, index) in triangle.iter().enumerate() {
                match shape.positions.get(*index as usize) {
                    Some(at) => {
                        for a in 0..3 {
                            p[k][a] = (at[a] - corner[a]) * 1000.0;
                        }
                    }
                    None => whole = false,
                }
            }
            if !whole {
                continue;
            }
            for corner in p {
                points.extend(corner.iter().map(|c| *c as f32));
                if several {
                    colours.extend(shape.triangle_colors[t].map(|c| c.clamp(0.0, 1.0)));
                }
            }
            faces += 1;
        }
        // the number the checker tells equal shapes by
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for value in &points {
            for byte in value.to_be_bytes() {
                hash = (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3);
            }
        }

        let class = self.class("BoundaryRepresentation");
        let id = self.entity(class, true);
        self.g.set(id, "Item", "parent", element);
        let origin = self.prop("ObjectProperty", id, 4, None, None, frame.into());
        self.g.set(id, "CoordinateSystem", "origin", origin);
        self.g
            .set(id, "GEntity", "hashValue", Val::Long(hash as i64));
        self.g.set(id, "GEntity", "versionOnStream", Val::Int(2));
        let colour = {
            let class = self.class("MaterialInfo");
            let c = self.g.object(class);
            let [red, green, blue, opacity] = shape.color.map(|v| v.clamp(0.0, 1.0));
            self.g.set(c, "MaterialInfo", "red", Val::Float(red));
            self.g.set(c, "MaterialInfo", "green", Val::Float(green));
            self.g.set(c, "MaterialInfo", "blue", Val::Float(blue));
            self.g
                .set(c, "MaterialInfo", "transparency", Val::Float(1.0 - opacity));
            c
        };
        self.g.set(id, "GEntity", "importedMaterialInfo", colour);
        let material = self.prop("ObjectProperty", id, 3, None, None, colour.into());
        self.g.set(id, "GEntity", "material", material);

        let floats = self.class_named("[F");
        let ints = self.class_named("[I");
        // the copy to draw from: no triangles, no normals, no colours
        self.g.extra(id, "GEntity", Val::Block(vec![0, 0, 0, 0]));
        let none = self.g.floats(floats, Vec::new());
        self.g.extra(id, "GEntity", none);
        let none = self.g.floats(floats, Vec::new());
        self.g.extra(id, "GEntity", none);
        self.g.extra(id, "GEntity", Val::Block(vec![0, 0, 0, 0]));
        // a colour for each corner of each face, when there are several
        if several {
            let array = self.g.floats(floats, colours);
            self.g.set(id, "BoundaryRepresentation", "colors", array);
        }

        self.g
            .set(id, "BoundaryRepresentation", "versionOnStream", Val::Int(5));
        // every face has one outline, of three points
        let contours = self.g.ints(ints, vec![1; faces]);
        self.g
            .set(id, "BoundaryRepresentation", "contours", contours);
        let strips = self.g.ints(ints, vec![3; faces]);
        self.g.set(id, "BoundaryRepresentation", "strips", strips);
        let vertices = self.g.floats(floats, points);
        self.g
            .set(id, "BoundaryRepresentation", "vertices", vertices);
        nodes.add(class, id);
        id
    }
}

/// A matrix that moves and does not turn: row by row, the move in the fourth column.
fn translation(by: [f64; 3]) -> [f64; 16] {
    [
        1.0, 0.0, 0.0, by[0], 0.0, 1.0, 0.0, by[1], 0.0, 0.0, 1.0, by[2], 0.0, 0.0, 0.0, 1.0,
    ]
}

/// The grey the checker gives a code it has no colour for.
const CODE_GREY: f32 = 150.0 / 255.0;

impl Builder {
    fn constant(&mut self, class: &str, name: &'static str) -> Id {
        let class = self.class_named(class);
        if let Some(id) = self.constants.get(&(class, name)) {
            return *id;
        }
        let id = self.g.constant(class, name);
        self.constants.insert((class, name), id);
        id
    }

    /// A map of texts to objects, as `java.util.HashMap` writes itself: the
    /// number of buckets and of entries, then each key and value.
    fn map(&mut self, entries: Vec<(Id, Id)>) -> Id {
        let mut buckets = 16usize;
        while buckets * 3 / 4 <= entries.len() {
            buckets *= 2;
        }
        let class = self.class("HashMap");
        let id = self.g.object(class);
        self.g.set(id, "HashMap", "loadFactor", Val::Float(0.75));
        self.g.set(
            id,
            "HashMap",
            "threshold",
            Val::Int((buckets * 3 / 4) as i32),
        );
        let mut sizes = (buckets as i32).to_be_bytes().to_vec();
        sizes.extend((entries.len() as i32).to_be_bytes());
        self.g.extra(id, "HashMap", Val::Block(sizes));
        for (key, value) in entries {
            self.g.extra(id, "HashMap", key);
            self.g.extra(id, "HashMap", value);
        }
        id
    }

    /// The type, the materials, the systems and the classification codes of
    /// an element: each is made the first time an element has it.
    fn more(&mut self, nodes: &mut Nodes, parts: &mut ModelParts, e: &WriteElement, id: Id) {
        if let Some(name) = e.type_name.as_deref().filter(|n| !n.is_empty()) {
            let ifc_class = e.type_class.clone().unwrap_or_default();
            let key = (ifc_class.to_ascii_lowercase(), name.to_string());
            let at = match parts.type_at.get(&key) {
                Some(at) => *at,
                None => {
                    let kind = self.type_object(nodes, &ifc_class, name);
                    parts.types.push((kind, Vec::new()));
                    parts.type_at.insert(key, parts.types.len() - 1);
                    parts.types.len() - 1
                }
            };
            parts.types[at].1.push(id);
            let kind = parts.types[at].0;
            let link = self.prop("ObjectProperty", id, 4, None, None, kind.into());
            self.g.set(id, "SEntity", "type", link);
        }

        let materials: Vec<String> = e
            .materials
            .iter()
            .filter(|m| !m.is_empty())
            .cloned()
            .collect();
        if !materials.is_empty() && self.g.has_part(id, "SElement") {
            let list = match parts.materials.get(&materials) {
                Some(list) => *list,
                None => {
                    let class = self.class("SMaterial");
                    let mut items = Vec::new();
                    for name in &materials {
                        let material = self.entity(class, true);
                        self.g
                            .set(material, "SMaterial", "isThicknessNull", Val::Bool(true));
                        let value = self.text("IfcMaterial");
                        self.g.set(material, "SMaterial", "ifcType", value);
                        let value = self.text(name);
                        let value =
                            self.prop("StringProperty", material, 1, None, None, value.into());
                        self.g.set(material, "SMaterial", "name", value);
                        let zero = self.zero;
                        let thickness =
                            self.prop("DoubleProperty", material, 1, None, None, zero.into());
                        self.g
                            .set(thickness, "DoubleProperty", "quantityType", Val::Int(1));
                        self.g.set(material, "SMaterial", "thickness", thickness);
                        nodes.add(class, material);
                        items.push(material.into());
                    }
                    let list = self.list(items);
                    parts.materials.insert(materials, list);
                    list
                }
            };
            let link = self.prop("ObjectProperty", id, 4, None, None, list.into());
            self.g.set(id, "SElement", "material", link);
        }

        for name in e.systems.iter().filter(|s| !s.is_empty()) {
            let at = match parts.system_at.get(name) {
                Some(at) => *at,
                None => {
                    let guid = self.guid();
                    let system = self.building_object(
                        nodes,
                        SYSTEM,
                        "IfcSystem",
                        &guid,
                        Some(name),
                        None,
                        None,
                        None,
                    );
                    let frame = self.frame(nodes, Some(system), None, Some(parts.frame));
                    let children = self.vector(vec![frame.into()]);
                    self.g.set(system, "Entity", "children", children);
                    self.g.set(system, "SEntity", "coordinateSystem", frame);
                    parts.systems.push((system, Vec::new()));
                    parts
                        .system_at
                        .insert(name.clone(), parts.systems.len() - 1);
                    parts.systems.len() - 1
                }
            };
            if !parts.systems[at].1.contains(&id) {
                parts.systems[at].1.push(id);
            }
        }

        let class = self.g.class_of(id);
        for c in e.classifications.iter().filter(|c| !c.code.is_empty()) {
            let system = if c.system.is_empty() {
                "Classification"
            } else {
                c.system.as_str()
            };
            let at = match self.classified_at.get(system) {
                Some(at) => *at,
                None => {
                    let of = self.class("Classification");
                    let object = self.entity(of, true);
                    self.classified.push(Classified {
                        object,
                        name: system.to_string(),
                        codes: Vec::new(),
                        code_at: HashMap::new(),
                        classes: Vec::new(),
                    });
                    self.classified_at
                        .insert(system.to_string(), self.classified.len() - 1);
                    self.classified.len() - 1
                }
            };
            let code = match self.classified[at].code_at.get(&c.code) {
                Some(code) => *code,
                None => {
                    let reference = self.code_reference(self.classified[at].object, c);
                    let system = &mut self.classified[at];
                    system.codes.push(Code {
                        code: c.code.clone(),
                        reference,
                        elements: Vec::new(),
                    });
                    system
                        .code_at
                        .insert(c.code.clone(), system.codes.len() - 1);
                    system.codes.len() - 1
                }
            };
            let system = &mut self.classified[at];
            if !system.codes[code].elements.contains(&id) {
                system.codes[code].elements.push(id);
            }
            if !system.classes.contains(&class) {
                system.classes.push(class);
            }
        }
    }

    /// The type of elements: its name, and what its class holds by default.
    fn type_object(&mut self, nodes: &mut Nodes, ifc_class: &str, name: &str) -> Id {
        let class = self
            .type_lower
            .get(&ifc_class.to_ascii_lowercase())
            .cloned()
            .unwrap_or_else(|| PLAIN_TYPE.to_string());
        let class = self.class_named(&class);
        let id = self.entity(class, true);
        let value = self.text_prop(id, 4, Some(name));
        self.g.set(id, "SType", "code", value);
        for field in ["description", "elementType", "predefinedType"] {
            let value = self.text_prop(id, 4, None);
            self.g.set(id, "SType", field, value);
        }
        let value = self.text(if ifc_class.is_empty() {
            "IfcTypeObject"
        } else {
            ifc_class
        });
        self.g.set(id, "SType", "ifcType", value);
        let value = self.prop("ObjectProperty", id, 0, None, None, Val::Null);
        self.g.set(id, "SType", "material", value);
        let guid = self.guid();
        let value = self.text(&guid);
        self.g.set(id, "SType", "uniqueId", value);
        self.defaults(id);
        nodes.add(class, id);
        id
    }

    /// A code of a classification system.
    fn code_reference(&mut self, system: Id, c: &super::WriteClassification) -> Id {
        let class = self.class("ClassificationReference");
        let id = self.entity(class, true);
        self.g
            .set(id, "ClassificationReference", "classification", system);
        let value = self.text(&c.name);
        self.g
            .set(id, "ClassificationReference", "description", value);
        let grey = {
            let class = self.class("MaterialInfo");
            let colour = self.g.object(class);
            for part in ["red", "green", "blue"] {
                self.g
                    .set(colour, "MaterialInfo", part, Val::Float(CODE_GREY));
            }
            colour
        };
        self.g.set(id, "ClassificationReference", "material", grey);
        let value = self.text(&c.code);
        self.g.set(id, "ClassificationReference", "name", value);
        let from_ifc = self.constant(CODE_SOURCE, "IFC");
        self.g
            .set(id, "ClassificationReference", "source", from_ifc);
        id
    }

    /// Fills in the classification systems, now that every element has been
    /// written, and files them and their codes at the root.
    fn classifications(&mut self, nodes: &mut Nodes) {
        let systems = std::mem::take(&mut self.classified);
        let system_class = self.class("Classification");
        let code_class = self.class("ClassificationReference");
        for system in systems {
            let id = system.object;
            for field in [
                "imported",
                "savable",
                "useFirstMatchMethod",
                "visualizeSpacesFlat",
            ] {
                self.g.set(id, "Classification", field, Val::Bool(true));
            }
            // the classes of the elements the system has codes for, as a list
            // and as the filters that let them in
            let class_array = self.class_named(CLASSES);
            let class_objects: Vec<Val> = system
                .classes
                .iter()
                .map(|c| self.g.class_object(*c).into())
                .collect();
            let classes = self.g.objects(class_array, class_objects);
            self.g.set(id, "Classification", "classes", classes);
            let mut filters = Vec::new();
            for class in &system.classes {
                let of = self.class("ClassAndPropertyFilter");
                let filter = self.g.object(of);
                self.g.set(
                    filter,
                    "ClassAndPropertyFilter",
                    "serializedVersion",
                    Val::Int(1),
                );
                let class_object = self.g.class_object(*class);
                self.g
                    .set(filter, "ClassAndPropertyFilter", "clazz", class_object);
                let include = self.constant(FILTER_STATE, "include");
                self.g
                    .set(filter, "ClassAndPropertyFilter", "filterState", include);
                self.g.extra(filter, "ClassAndPropertyFilter", Val::Null);
                filters.push(filter.into());
            }
            let filters = self.vector(filters);
            self.g.set(id, "Classification", "filters", filters);

            // the columns the checker shows a classification with
            let column = {
                let class = self.class("ComponentClassificationColumn");
                let column = self.g.object(class);
                let name = self.text("Component");
                self.g.set(column, "ClassificationColumn", "name", name);
                column
            };
            let mut columns = vec![column.into()];
            for (name, ordinal) in [("Type", 1), ("Layer", 5), ("Name", 4)] {
                let class = self.class("PropertyClassificationColumn");
                let column = self.g.object(class);
                let name = self.text(name);
                self.g.set(column, "ClassificationColumn", "name", name);
                let class = self.class("IdentificationType");
                let kind = self.g.object(class);
                self.g
                    .set(kind, "IdentificationType", "ordinal", Val::Int(ordinal));
                let class = self.class("IdentificationPropertyReference");
                let reference = self.g.object(class);
                self.g
                    .set(reference, "IdentificationPropertyReference", "type", kind);
                self.g.set(
                    column,
                    "PropertyClassificationColumn",
                    "propertyRef",
                    reference,
                );
                columns.push(column.into());
            }
            let columns = self.list(columns);
            self.g
                .set(id, "Classification", "classificationColumns", columns);

            let data = self.vector(Vec::new());
            self.g.set(id, "Classification", "data", data);
            let mut rows = Vec::new();
            let mut by_code = Vec::new();
            for code in &system.codes {
                let text = self.text(&code.code);
                let row = self.vector(vec![text.into(), Val::Null]);
                rows.push(row.into());
                let list = self.list(vec![code.reference.into()]);
                by_code.push((text, list));
            }
            let rows = self.vector(rows);
            self.g.set(id, "Classification", "defaultData", rows);
            let references = self.map(by_code);
            self.g.set(id, "Classification", "references", references);

            let description = self.text(&format!(
                "<html><body>Name: {}<br>Location: Undefined<br></body></html>",
                system.name
            ));
            self.g.set(id, "Classification", "description", description);
            let disciplines = self.class_named(DISCIPLINES);
            let disciplines = self.g.objects(disciplines, Vec::new());
            self.g.set(id, "Classification", "disciplines", disciplines);
            let name = self.text(&system.name);
            self.g.set(id, "Classification", "name", name);
            let source = self.text("");
            self.g.set(id, "Classification", "source", source);

            nodes.add(system_class, id);
            for code in system.codes {
                nodes.add(code_class, code.reference);
                self.relation("Classifies", code.reference, None, code.elements);
            }
        }
    }
}
