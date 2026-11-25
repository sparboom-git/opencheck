//! The classes of a coordination file: their names, version numbers and
//! fields, which IFC class is written as which of them, and what the fields
//! of an element's own classes hold when nothing is known about them.
//!
//! The table (`classes.txt`) is drawn out of files a model checker saved of
//! OpenCheck's own test models, by `testdata/smc-test/make_class_table.py`.
//! A writer cannot make these up: a file whose class has another version
//! number than the checker's own class is refused whole.

use std::collections::HashMap;

use super::java::{ClassId, Graph};

const TABLE: &str = include_str!("classes.txt");

/// The name of the checker's maker, as the checker's class names have it.
/// The table and this source write `~` for it, so the name stands nowhere
/// in OpenCheck; the files written have it, since the format does.
fn maker() -> String {
    String::from_utf8_lossy(&[115, 111, 108, 105, 98, 114, 105]).into_owned()
}

/// A class name or type text in full, from one written with `~`.
pub fn full(name: &str) -> String {
    name.replace('~', &maker())
}

/// What a field holds when nothing is known about it.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Null,
    Bool(bool),
    Int(i32),
    Num(f64),
    /// A property object: its class, its status, the kind of quantity (of a
    /// number) and its value.
    Prop {
        class: String,
        status: i32,
        quantity: Option<i32>,
        value: PropValue,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Null,
    Text(String),
    Double(f64),
    Integer(i32),
    Boolean(bool),
    /// An object of plain numbers: its class and the numbers.
    Object(String, Vec<i32>),
}

pub struct Table {
    /// IFC class -> the class it is written as.
    pub written_as: HashMap<String, String>,
    /// Class -> its fields with what they hold by default, in the table's order.
    pub defaults: HashMap<String, Vec<(String, Shape)>>,
}

/// Reads the table: the classes into the graph, the rest as it is.
pub fn load(graph: &mut Graph) -> Table {
    let mut rows: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut order = Vec::new();
    let mut table = Table {
        written_as: HashMap::new(),
        defaults: HashMap::new(),
    };
    let text = full(TABLE);
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        match cols[0] {
            "C" => {
                order.push(cols[1]);
                rows.insert(cols[1], cols);
            }
            "M" => {
                table
                    .written_as
                    .insert(cols[1].to_string(), cols[2].to_string());
            }
            "D" => table
                .defaults
                .entry(cols[1].to_string())
                .or_default()
                .push((cols[2].to_string(), shape(cols[3]))),
            _ => {}
        }
    }
    for name in order {
        add(graph, &rows, name);
    }
    table
}

/// Adds a class after the one it extends.
fn add(graph: &mut Graph, rows: &HashMap<&str, Vec<&str>>, name: &str) -> ClassId {
    if let Some(id) = graph.class_named(name) {
        return id;
    }
    let cols = &rows[name];
    let parent = (cols[4] != "-").then(|| add(graph, rows, cols[4]));
    let fields: Vec<(u8, &str, Option<&str>)> = cols[5..]
        .iter()
        .map(|field| {
            let mut parts = field.splitn(3, ' ');
            let ty = parts.next().unwrap().as_bytes()[0];
            (ty, parts.next().unwrap(), parts.next())
        })
        .collect();
    let uid: i64 = cols[2].parse().expect("a version number");
    let flags: u8 = cols[3].parse().expect("flags");
    graph.add_class(name, uid, flags, parent, &fields)
}

fn shape(text: &str) -> Shape {
    let words: Vec<&str> = text.split(' ').collect();
    match words[0] {
        "bool" => Shape::Bool(words[1] == "true"),
        "int" => Shape::Int(words[1].parse().unwrap_or(0)),
        "num" => Shape::Num(words[1].parse().unwrap_or(0.0)),
        "prop" => {
            let class = words[1].to_string();
            let status = words[2].parse().unwrap_or(0);
            let (quantity, rest) = if class == "DoubleProperty" {
                (words[3].parse().ok(), &words[4..])
            } else {
                (None, &words[3..])
            };
            let value = match rest.first().copied() {
                Some("text") => PropValue::Text(rest[1..].join(" ")),
                Some("double") => PropValue::Double(rest[1].parse().unwrap_or(0.0)),
                Some("integer") => PropValue::Integer(rest[1].parse().unwrap_or(0)),
                Some("boolean") => PropValue::Boolean(rest[1] == "true"),
                Some("object") => PropValue::Object(
                    rest[1].to_string(),
                    rest[2..].iter().filter_map(|n| n.parse().ok()).collect(),
                ),
                _ => PropValue::Null,
            };
            Shape::Prop {
                class,
                status,
                quantity,
                value,
            }
        }
        _ => Shape::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_the_classes_a_file_needs() {
        let mut graph = Graph::default();
        let table = load(&mut graph);
        for short in [
            "SModel",
            "SWall",
            "SBuildingStorey",
            "BoundaryRepresentation",
            "CoordinateSystem",
            "PropertySet",
            "SContains",
            "SHasGeometry",
            "ArrayList",
            "Vector",
            "UUID",
        ] {
            assert!(graph.class_short(short).is_some(), "{short}");
        }
        let wall = graph.class(graph.class_short("SWall").unwrap());
        // SWall, SElement, SProduct, SEntity, Entity, Item
        assert_eq!(wall.chain.len(), 6);
        assert_eq!(wall.uid, 3704234900238282295);
        assert_eq!(
            table.written_as["IfcWall"],
            full("com.~.sae.~.construction.SWall")
        );
        assert_eq!(
            table.written_as["IfcBuildingElementProxy"],
            full("com.~.sae.~.construction.SObject")
        );
        let thickness = table.defaults[&full("com.~.sae.~.construction.SWall")]
            .iter()
            .find(|(field, _)| field == "thickness")
            .unwrap();
        assert_eq!(
            thickness.1,
            Shape::Prop {
                class: "DoubleProperty".into(),
                status: 1,
                quantity: Some(1),
                value: PropValue::Double(0.0),
            }
        );
    }
}
