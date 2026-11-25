//! A graph of Java objects written as `java.io.ObjectOutputStream` writes it
//! (protocol 2, the Java Object Serialization Specification, chapter 6).
//!
//! The graph knows nothing of what the classes mean: an object is a class and
//! the values of its fields, class by class from the topmost down, and what
//! the class wrote after them. The writer hands out the handles as Java does:
//! an object, a text or a class is written in full the first time it is met
//! and as a reference after that, so objects may refer to each other in
//! circles.

use std::collections::HashMap;

/// An object, a text or an array in the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id(u32);

/// A class in the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClassId(u32);

/// The value of a field, or what a class wrote after its fields.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Byte(i8),
    Char(u16),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Ref(Id),
    /// Raw bytes a class wrote itself (only after the fields).
    Block(Vec<u8>),
}

impl From<Id> for Val {
    fn from(id: Id) -> Val {
        Val::Ref(id)
    }
}

impl From<Option<Id>> for Val {
    fn from(id: Option<Id>) -> Val {
        id.map_or(Val::Null, Val::Ref)
    }
}

pub struct Field {
    /// The type code of the field: `I`, `Z`, `D` ..., `L` for an object, `[` for an array.
    pub ty: u8,
    pub name: String,
    /// For an object or an array, the text naming its type (`Ljava/lang/String;`).
    type_text: Option<Id>,
}

pub struct Class {
    pub name: String,
    pub uid: i64,
    pub flags: u8,
    pub parent: Option<ClassId>,
    pub fields: Vec<Field>,
    /// The class and those it extends, the topmost first.
    pub chain: Vec<ClassId>,
}

impl Class {
    /// The last part of the name: `SWall` of a wall's class.
    pub fn short(&self) -> &str {
        self.name.rsplit('.').next().unwrap_or(&self.name)
    }

    /// Whether the class writes something of its own after its fields.
    fn writes_itself(&self) -> bool {
        self.flags & 0x01 != 0
    }
}

/// The part of an object one of its classes holds.
struct Part {
    class: ClassId,
    fields: Vec<Val>,
    /// What the class wrote after its fields, when it writes itself.
    extra: Option<Vec<Val>>,
}

enum Node {
    Text(String),
    /// A `java.lang.Class` object.
    ClassOf(ClassId),
    Object {
        class: ClassId,
        parts: Vec<Part>,
    },
    Objects {
        class: ClassId,
        items: Vec<Val>,
    },
    Floats {
        class: ClassId,
        items: Vec<f32>,
    },
    Doubles {
        class: ClassId,
        items: Vec<f64>,
    },
    Ints {
        class: ClassId,
        items: Vec<i32>,
    },
}

#[derive(Default)]
pub struct Graph {
    nodes: Vec<Node>,
    classes: Vec<Class>,
    by_name: HashMap<String, ClassId>,
    by_short: HashMap<String, ClassId>,
    type_texts: HashMap<String, Id>,
}

impl Graph {
    pub fn class(&self, id: ClassId) -> &Class {
        &self.classes[id.0 as usize]
    }

    /// A class by its full name.
    pub fn class_named(&self, name: &str) -> Option<ClassId> {
        self.by_name.get(name).copied()
    }

    /// A class by the last part of its name; the first one of that name.
    pub fn class_short(&self, short: &str) -> Option<ClassId> {
        self.by_short.get(short).copied()
    }

    /// Adds a class. The class it extends must be there already; fields are
    /// (type code, name, type text).
    pub fn add_class(
        &mut self,
        name: &str,
        uid: i64,
        flags: u8,
        parent: Option<ClassId>,
        fields: &[(u8, &str, Option<&str>)],
    ) -> ClassId {
        let id = ClassId(self.classes.len() as u32);
        let fields = fields
            .iter()
            .map(|(ty, field, type_text)| Field {
                ty: *ty,
                name: (*field).to_string(),
                type_text: type_text.map(|t| self.type_text(t)),
            })
            .collect();
        let mut chain = parent.map_or_else(Vec::new, |p| self.class(p).chain.clone());
        chain.push(id);
        let class = Class {
            name: name.to_string(),
            uid,
            flags,
            parent,
            fields,
            chain,
        };
        self.by_short.entry(class.short().to_string()).or_insert(id);
        self.by_name.insert(name.to_string(), id);
        self.classes.push(class);
        id
    }

    /// The text of a type, one object per text, as Java writes them.
    fn type_text(&mut self, text: &str) -> Id {
        if let Some(id) = self.type_texts.get(text) {
            return *id;
        }
        let id = self.text(text);
        self.type_texts.insert(text.to_string(), id);
        id
    }

    fn push(&mut self, node: Node) -> Id {
        self.nodes.push(node);
        Id(self.nodes.len() as u32 - 1)
    }

    pub fn text(&mut self, text: &str) -> Id {
        self.push(Node::Text(text.to_string()))
    }

    /// The `java.lang.Class` object of a class.
    pub fn class_object(&mut self, class: ClassId) -> Id {
        self.push(Node::ClassOf(class))
    }

    /// An object of a class with every field empty: numbers zero, objects null.
    pub fn object(&mut self, class: ClassId) -> Id {
        let parts = self
            .class(class)
            .chain
            .iter()
            .filter(|c| self.class(**c).flags & 0x02 != 0)
            .map(|c| {
                let of = self.class(*c);
                Part {
                    class: *c,
                    fields: of
                        .fields
                        .iter()
                        .map(|f| match f.ty {
                            b'Z' => Val::Bool(false),
                            b'B' => Val::Byte(0),
                            b'C' => Val::Char(0),
                            b'S' => Val::Short(0),
                            b'I' => Val::Int(0),
                            b'J' => Val::Long(0),
                            b'F' => Val::Float(0.0),
                            b'D' => Val::Double(0.0),
                            _ => Val::Null,
                        })
                        .collect(),
                    extra: of.writes_itself().then(Vec::new),
                }
            })
            .collect();
        self.push(Node::Object { class, parts })
    }

    pub fn class_of(&self, object: Id) -> ClassId {
        match &self.nodes[object.0 as usize] {
            Node::Object { class, .. } => *class,
            _ => panic!("not an object"),
        }
    }

    /// Sets a field of an object: the class that has it by the last part of
    /// its name, and the field. Panics when the object has no such field,
    /// which is a mistake in the writer and not in what is written.
    pub fn set(&mut self, object: Id, class: &str, field: &str, value: impl Into<Val>) {
        let (part, at) = self.find(object, class, field);
        let Node::Object { parts, .. } = &mut self.nodes[object.0 as usize] else {
            unreachable!()
        };
        parts[part].fields[at] = value.into();
    }

    fn find(&self, object: Id, class: &str, field: &str) -> (usize, usize) {
        let Node::Object { parts, .. } = &self.nodes[object.0 as usize] else {
            panic!("{class}.{field}: not an object");
        };
        for (p, part) in parts.iter().enumerate() {
            let of = self.class(part.class);
            if of.short() == class
                && let Some(at) = of.fields.iter().position(|f| f.name == field)
            {
                return (p, at);
            }
        }
        panic!("an object has no field {class}.{field}");
    }

    /// Adds to what a class of the object writes after its fields.
    pub fn extra(&mut self, object: Id, class: &str, value: impl Into<Val>) {
        let at = {
            let Node::Object { parts, .. } = &self.nodes[object.0 as usize] else {
                panic!("{class}: not an object");
            };
            parts
                .iter()
                .position(|p| self.class(p.class).short() == class)
                .unwrap_or_else(|| panic!("an object has no class {class}"))
        };
        let Node::Object { parts, .. } = &mut self.nodes[object.0 as usize] else {
            unreachable!()
        };
        parts[at]
            .extra
            .as_mut()
            .unwrap_or_else(|| panic!("{class} writes nothing of its own"))
            .push(value.into());
    }

    pub fn objects(&mut self, class: ClassId, items: Vec<Val>) -> Id {
        self.push(Node::Objects { class, items })
    }

    pub fn floats(&mut self, class: ClassId, items: Vec<f32>) -> Id {
        self.push(Node::Floats { class, items })
    }

    pub fn doubles(&mut self, class: ClassId, items: Vec<f64>) -> Id {
        self.push(Node::Doubles { class, items })
    }

    pub fn ints(&mut self, class: ClassId, items: Vec<i32>) -> Id {
        self.push(Node::Ints { class, items })
    }

    /// The stream of the graph as it is reached from `top`.
    pub fn write(&self, top: Id) -> Vec<u8> {
        let mut w = Writer {
            graph: self,
            out: vec![0xac, 0xed, 0x00, 0x05],
            nodes: vec![NONE; self.nodes.len()],
            classes: vec![NONE; self.classes.len()],
            next: 0,
        };
        w.node(top);
        w.out
    }
}

const BASE: u32 = 0x7e_0000;
const NONE: u32 = u32::MAX;

struct Writer<'a> {
    graph: &'a Graph,
    out: Vec<u8>,
    /// The handle each node and class got when it was written.
    nodes: Vec<u32>,
    classes: Vec<u32>,
    next: u32,
}

impl Writer<'_> {
    fn handle(&mut self) -> u32 {
        self.next += 1;
        self.next - 1
    }

    fn reference(&mut self, handle: u32) {
        self.out.push(0x71);
        self.out.extend_from_slice(&(handle + BASE).to_be_bytes());
    }

    fn utf(&mut self, text: &str) {
        let bytes = modified_utf8(text);
        self.out
            .extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        self.out.extend_from_slice(&bytes);
    }

    fn class_desc(&mut self, class: Option<ClassId>) {
        let Some(class) = class else {
            self.out.push(0x70);
            return;
        };
        if self.classes[class.0 as usize] != NONE {
            self.reference(self.classes[class.0 as usize]);
            return;
        }
        let graph = self.graph;
        let of = graph.class(class);
        self.out.push(0x72);
        self.utf(&of.name);
        self.out.extend_from_slice(&of.uid.to_be_bytes());
        self.classes[class.0 as usize] = self.handle();
        self.out.push(of.flags);
        self.out
            .extend_from_slice(&(of.fields.len() as u16).to_be_bytes());
        for field in &of.fields {
            self.out.push(field.ty);
            self.utf(&field.name);
            if let Some(text) = field.type_text {
                self.node(text);
            }
        }
        self.out.push(0x78);
        self.class_desc(of.parent);
    }

    fn value(&mut self, value: &Val) {
        match value {
            Val::Null => self.out.push(0x70),
            Val::Ref(id) => self.node(*id),
            Val::Block(bytes) => {
                if bytes.len() < 256 {
                    self.out.push(0x77);
                    self.out.push(bytes.len() as u8);
                } else {
                    self.out.push(0x7a);
                    self.out
                        .extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                }
                self.out.extend_from_slice(bytes);
            }
            other => panic!("{other:?} where an object was expected"),
        }
    }

    fn field(&mut self, ty: u8, value: &Val) {
        match (ty, value) {
            (b'Z', Val::Bool(v)) => self.out.push(*v as u8),
            (b'B', Val::Byte(v)) => self.out.push(*v as u8),
            (b'C', Val::Char(v)) => self.out.extend_from_slice(&v.to_be_bytes()),
            (b'S', Val::Short(v)) => self.out.extend_from_slice(&v.to_be_bytes()),
            (b'I', Val::Int(v)) => self.out.extend_from_slice(&v.to_be_bytes()),
            (b'J', Val::Long(v)) => self.out.extend_from_slice(&v.to_be_bytes()),
            (b'F', Val::Float(v)) => self.out.extend_from_slice(&v.to_be_bytes()),
            (b'D', Val::Double(v)) => self.out.extend_from_slice(&v.to_be_bytes()),
            (b'L' | b'[', Val::Null | Val::Ref(_)) => self.value(value),
            (ty, value) => panic!("{value:?} in a field of type {}", ty as char),
        }
    }

    fn node(&mut self, id: Id) {
        if self.nodes[id.0 as usize] != NONE {
            self.reference(self.nodes[id.0 as usize]);
            return;
        }
        let graph = self.graph;
        match &graph.nodes[id.0 as usize] {
            Node::Text(text) => {
                self.nodes[id.0 as usize] = self.handle();
                let bytes = modified_utf8(text);
                if bytes.len() <= 0xffff {
                    self.out.push(0x74);
                    self.out
                        .extend_from_slice(&(bytes.len() as u16).to_be_bytes());
                } else {
                    self.out.push(0x7c);
                    self.out
                        .extend_from_slice(&(bytes.len() as u64).to_be_bytes());
                }
                self.out.extend_from_slice(&bytes);
            }
            Node::ClassOf(class) => {
                self.out.push(0x76);
                self.class_desc(Some(*class));
                self.nodes[id.0 as usize] = self.handle();
            }
            Node::Object { class, parts } => {
                self.out.push(0x73);
                self.class_desc(Some(*class));
                self.nodes[id.0 as usize] = self.handle();
                for part in parts {
                    let of = graph.class(part.class);
                    for (field, value) in of.fields.iter().zip(&part.fields) {
                        self.field(field.ty, value);
                    }
                    if let Some(extra) = &part.extra {
                        for value in extra {
                            self.value(value);
                        }
                        self.out.push(0x78);
                    }
                }
            }
            Node::Objects { class, items } => {
                self.array(id, *class, items.len());
                for item in items {
                    self.value(item);
                }
            }
            Node::Floats { class, items } => {
                self.array(id, *class, items.len());
                for v in items {
                    self.out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Node::Doubles { class, items } => {
                self.array(id, *class, items.len());
                for v in items {
                    self.out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Node::Ints { class, items } => {
                self.array(id, *class, items.len());
                for v in items {
                    self.out.extend_from_slice(&v.to_be_bytes());
                }
            }
        }
    }

    fn array(&mut self, id: Id, class: ClassId, len: usize) {
        self.out.push(0x75);
        self.class_desc(Some(class));
        self.nodes[id.0 as usize] = self.handle();
        self.out.extend_from_slice(&(len as i32).to_be_bytes());
    }
}

/// A text as Java writes it: UTF-8, but the zero character in two bytes and a
/// character outside the basic plane as its two halves, three bytes each.
fn modified_utf8(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for unit in text.encode_utf16() {
        match unit {
            0x0001..=0x007f => out.push(unit as u8),
            0x0000 | 0x0080..=0x07ff => {
                out.push(0xc0 | (unit >> 6) as u8);
                out.push(0x80 | (unit & 0x3f) as u8);
            }
            _ => {
                out.push(0xe0 | (unit >> 12) as u8);
                out.push(0x80 | ((unit >> 6) & 0x3f) as u8);
                out.push(0x80 | (unit & 0x3f) as u8);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_is_written_as_java_writes_it() {
        assert_eq!(modified_utf8("Væg"), [b'V', 0xc3, 0xa6, b'g']);
        assert_eq!(modified_utf8("\0"), [0xc0, 0x80]);
        // a character outside the basic plane: two halves of three bytes
        assert_eq!(modified_utf8("\u{1F600}").len(), 6);
    }

    #[test]
    fn an_object_met_twice_is_a_reference_the_second_time() {
        let mut g = Graph::default();
        let list = g.add_class(
            "java.util.ArrayList",
            8683452581122892189,
            3,
            None,
            &[(b'I', "size", None)],
        );
        let object = g.add_class("[Ljava.lang.Object;", -8012369246846506644, 2, None, &[]);
        let text = g.text("a");
        let inner = g.object(list);
        g.set(inner, "ArrayList", "size", Val::Int(1));
        g.extra(inner, "ArrayList", Val::Block(vec![0, 0, 0, 1]));
        g.extra(inner, "ArrayList", text);
        let both = g.objects(object, vec![inner.into(), inner.into(), text.into()]);
        let bytes = g.write(both);
        assert_eq!(&bytes[..4], [0xac, 0xed, 0, 5]);
        // the array is handle 1 (its class is 0), the list 3 (its class 2), the text 4
        let tail = &bytes[bytes.len() - 10..];
        assert_eq!(tail[..5], [0x71, 0x00, 0x7e, 0x00, 0x03]);
        assert_eq!(tail[5..], [0x71, 0x00, 0x7e, 0x00, 0x04]);
    }
}
