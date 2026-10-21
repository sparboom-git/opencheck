//! buildingSMART Information Delivery Specification (IDS) 1.0.
//!
//! An IDS file lists specifications. Each says which elements it applies to
//! (applicability) and what those elements must carry (requirements), both
//! written as facets: entity, attribute, property, classification, material
//! and partOf. Values are a simple value or an XML Schema restriction
//! (enumeration, pattern, bounds, length).
//!
//! This module reads and writes IDS XML and checks [`IfcModel`] elements
//! against it. Results take the same form as the built-in information
//! standards: one finding per element and specification, and a fill rate per
//! specification. What OpenCheck cannot evaluate is listed in
//! `docs/information-requirements.md`.

use std::fmt::{self, Write as _};

use quick_xml::Reader;
use quick_xml::events::Event;
use rayon::prelude::*;
use regex::Regex;
use rustc_hash::FxHashMap;

use super::engine::{CheckJob, CheckModel, Finding, FindingElement, FindingKind, finding_id};
use super::information::{InformationSummary, ItemCoverage, PartCoverage};
use super::ruleset::Rule;
use crate::{Aabb, Element, IfcModel, PropertyValue};

/// Prefix of a rule's `standard` that names an IDS file instead of a
/// built-in standard, e.g. `ids:requirements/delivery.ids`.
pub const IDS_PREFIX: &str = "ids:";

/// The IDS file path a rule's `standard` refers to, if it refers to one.
pub fn ids_path(standard: &str) -> Option<&str> {
    standard
        .strip_prefix(IDS_PREFIX)
        .map(str::trim)
        .filter(|p| !p.is_empty())
}

const IDS_NAMESPACE: &str = "http://standards.buildingsmart.org/IDS";
const XS_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema";

#[derive(Debug, thiserror::Error)]
pub enum IdsError {
    #[error("not valid XML: {0}")]
    Xml(String),
    #[error("not a valid IDS file: {0}")]
    Invalid(String),
}

fn invalid<T>(message: impl Into<String>) -> Result<T, IdsError> {
    Err(IdsError::Invalid(message.into()))
}

// ---- Model ------------------------------------------------------------------

/// A parsed IDS document.
#[derive(Debug, Clone, Default)]
pub struct Ids {
    pub info: IdsInfo,
    pub specifications: Vec<Specification>,
}

/// The `<info>` block. Only the title is required.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IdsInfo {
    pub title: String,
    pub copyright: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub date: String,
    pub purpose: String,
    pub milestone: String,
}

#[derive(Debug, Clone, Default)]
pub struct Specification {
    pub name: String,
    pub identifier: String,
    pub description: String,
    pub instructions: String,
    /// `IFC2X3`, `IFC4`, `IFC4X3_ADD2`. Models of other versions are skipped.
    pub ifc_versions: Vec<String>,
    /// Applicability `minOccurs`: 0 makes the specification optional; 1 or
    /// more requires that many applicable elements.
    pub min_occurs: u32,
    /// Applicability `maxOccurs`; None is unbounded. 0 prohibits applicable
    /// elements altogether.
    pub max_occurs: Option<u32>,
    /// All facets must match for an element to be checked.
    pub applicability: Vec<Facet>,
    pub requirements_description: String,
    pub requirements: Vec<Facet>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cardinality {
    /// The facet must match.
    #[default]
    Required,
    /// When the information is there, it must match.
    Optional,
    /// The facet must not match.
    Prohibited,
}

impl Cardinality {
    fn parse(text: Option<&str>) -> Result<Self, IdsError> {
        match text.map(str::trim) {
            None | Some("required") => Ok(Self::Required),
            Some("optional") => Ok(Self::Optional),
            Some("prohibited") => Ok(Self::Prohibited),
            Some(other) => invalid(format!("unknown cardinality '{other}'")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Prohibited => "prohibited",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Facet {
    pub kind: FacetKind,
    /// Requirements only; applicability facets are always required.
    pub cardinality: Cardinality,
    pub instructions: String,
    pub uri: String,
}

#[derive(Debug, Clone)]
pub enum FacetKind {
    /// IFC class (upper case in IDS, e.g. `IFCWALL`) and predefined type.
    Entity(EntityFacet),
    Attribute {
        name: IdsValue,
        value: Option<IdsValue>,
    },
    Property {
        property_set: IdsValue,
        base_name: IdsValue,
        /// IFC measure type in upper case, e.g. `IFCLABEL`.
        data_type: Option<String>,
        value: Option<IdsValue>,
    },
    Classification {
        system: Option<IdsValue>,
        value: Option<IdsValue>,
    },
    Material {
        value: Option<IdsValue>,
    },
    PartOf {
        /// `IFCRELAGGREGATES`, `IFCRELCONTAINEDINSPATIALSTRUCTURE`, ...;
        /// None is any relation.
        relation: Option<String>,
        entity: EntityFacet,
    },
}

#[derive(Debug, Clone)]
pub struct EntityFacet {
    pub name: IdsValue,
    pub predefined_type: Option<IdsValue>,
}

/// A value to match: one exact value or an XML Schema restriction.
#[derive(Debug, Clone)]
pub enum IdsValue {
    Simple(String),
    Restriction(Restriction),
}

/// `xs:restriction`. Every kind of constraint that is present must hold;
/// several enumeration values or patterns are alternatives.
#[derive(Debug, Clone, Default)]
pub struct Restriction {
    /// `xs:string`, `xs:double`, `xs:integer`, `xs:boolean`, ...
    pub base: String,
    pub enumeration: Vec<String>,
    /// XML Schema patterns, as written, with their compiled form.
    pub patterns: Vec<(String, Regex)>,
    pub min_inclusive: Option<f64>,
    pub max_inclusive: Option<f64>,
    pub min_exclusive: Option<f64>,
    pub max_exclusive: Option<f64>,
    pub length: Option<usize>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
}

impl Restriction {
    /// A restriction with one pattern; fails when the pattern does not compile.
    pub fn pattern(pattern: &str) -> Result<Self, IdsError> {
        Ok(Self {
            base: "xs:string".to_owned(),
            patterns: vec![(pattern.to_owned(), compile_pattern(pattern)?)],
            ..Self::default()
        })
    }

    pub fn enumeration(values: impl IntoIterator<Item = String>) -> Self {
        Self {
            base: "xs:string".to_owned(),
            enumeration: values.into_iter().collect(),
            ..Self::default()
        }
    }
}

/// Compiles an XML Schema pattern. XSD patterns always match the whole
/// value, so they are anchored here.
pub fn compile_pattern(pattern: &str) -> Result<Regex, IdsError> {
    // XSD name classes have no Rust equivalent; spell them out.
    let translated = pattern
        .replace(r"\i", "[_:A-Za-z]")
        .replace(r"\c", "[-._:A-Za-z0-9]");
    Regex::new(&format!("^(?:{translated})$"))
        .map_err(|e| IdsError::Invalid(format!("pattern {pattern:?}: {e}")))
}

// ---- Matching values ----------------------------------------------------------

/// Tolerance for real numbers, as IDS 1.0 prescribes: a value matches an
/// expected `e` within `1e-6 + 1e-6 * |e|`.
const REAL_TOLERANCE: f64 = 1e-6;

fn reals_equal(actual: f64, expected: f64) -> bool {
    let tolerance = REAL_TOLERANCE + REAL_TOLERANCE * expected.abs();
    // Allow for the rounding of values that sit exactly on the edge.
    (actual - expected).abs() <= tolerance * (1.0 + 1e-9)
}

impl IdsValue {
    /// Matches a text, case-sensitive.
    pub fn matches_text(&self, text: &str) -> bool {
        match self {
            IdsValue::Simple(s) => s == text,
            IdsValue::Restriction(r) => r.matches(&Scalar::Text(text)),
        }
    }

    /// Matches an IFC class name, which IDS writes in upper case
    /// (`IFCWALL`; `IfcWall` does not match).
    fn matches_name(&self, name: &str) -> bool {
        self.matches_text(&name.to_ascii_uppercase())
    }

    /// Matches a property or attribute value. `scale` converts numbers to SI
    /// units (lengths in millimetre models), since IDS values are in SI.
    pub fn matches_value(&self, value: &PropertyValue, scale: f64) -> bool {
        match value {
            PropertyValue::List(items) => items.iter().any(|v| self.matches_value(v, scale)),
            PropertyValue::Range { lower, upper } => {
                self.matches_value(lower, scale) || self.matches_value(upper, scale)
            }
            PropertyValue::Null | PropertyValue::Logical(None) => false,
            PropertyValue::Text(s) => self.matches_scalar(&Scalar::Text(s)),
            PropertyValue::Integer(i) => {
                if scale == 1.0 {
                    self.matches_scalar(&Scalar::Integer(*i))
                } else {
                    self.matches_scalar(&Scalar::Real(*i as f64 * scale))
                }
            }
            PropertyValue::Real(r) => self.matches_scalar(&Scalar::Real(r * scale)),
            PropertyValue::Boolean(b) | PropertyValue::Logical(Some(b)) => {
                self.matches_scalar(&Scalar::Boolean(*b))
            }
        }
    }

    fn matches_scalar(&self, scalar: &Scalar) -> bool {
        match self {
            IdsValue::Simple(s) => scalar.equals(s),
            IdsValue::Restriction(r) => r.matches(scalar),
        }
    }
}

/// A single model value in the form restrictions compare.
enum Scalar<'a> {
    Text(&'a str),
    Integer(i64),
    Real(f64),
    Boolean(bool),
}

impl Scalar<'_> {
    fn number(&self) -> Option<f64> {
        match self {
            Scalar::Integer(i) => Some(*i as f64),
            Scalar::Real(r) => Some(*r),
            Scalar::Text(_) | Scalar::Boolean(_) => None,
        }
    }

    fn text(&self) -> String {
        match self {
            Scalar::Text(s) => (*s).to_owned(),
            Scalar::Integer(i) => i.to_string(),
            Scalar::Real(r) => PropertyValue::Real(*r).to_string(),
            Scalar::Boolean(b) => b.to_string(),
        }
    }

    /// Equality with a value written in the IDS file.
    fn equals(&self, expected: &str) -> bool {
        let expected = expected.trim();
        match self {
            Scalar::Text(s) => *s == expected,
            // Booleans are lower case in IDS: `TRUE` matches nothing.
            Scalar::Boolean(b) => match expected {
                "true" | "1" => *b,
                "false" | "0" => !*b,
                _ => false,
            },
            // An integer is never written with a decimal point (`42.0`).
            Scalar::Integer(i) => expected.parse::<i64>().is_ok_and(|e| e == *i),
            Scalar::Real(_) => expected
                .parse::<f64>()
                .is_ok_and(|e| reals_equal(self.number().expect("numeric"), e)),
        }
    }
}

impl Restriction {
    fn matches(&self, value: &Scalar) -> bool {
        if !self.enumeration.is_empty() && !self.enumeration.iter().any(|e| value.equals(e)) {
            return false;
        }
        if !self.patterns.is_empty() {
            // Patterns only apply to texts, never to numbers or booleans.
            let Scalar::Text(text) = value else {
                return false;
            };
            if !self.patterns.iter().any(|(_, re)| re.is_match(text)) {
                return false;
            }
        }
        let bounded = self.min_inclusive.is_some()
            || self.max_inclusive.is_some()
            || self.min_exclusive.is_some()
            || self.max_exclusive.is_some();
        if bounded {
            let number = match value {
                Scalar::Text(s) => s.trim().parse::<f64>().ok(),
                other => other.number(),
            };
            let Some(n) = number else {
                return false;
            };
            // Bounds are exact: IDS applies no tolerance to them.
            if self.min_inclusive.is_some_and(|b| n < b)
                || self.max_inclusive.is_some_and(|b| n > b)
                || self.min_exclusive.is_some_and(|b| n <= b)
                || self.max_exclusive.is_some_and(|b| n >= b)
            {
                return false;
            }
        }
        if self.length.is_some() || self.min_length.is_some() || self.max_length.is_some() {
            let length = value.text().chars().count();
            if self.length.is_some_and(|l| length != l)
                || self.min_length.is_some_and(|l| length < l)
                || self.max_length.is_some_and(|l| length > l)
            {
                return false;
            }
        }
        true
    }
}

impl fmt::Display for IdsValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdsValue::Simple(s) => f.write_str(s),
            IdsValue::Restriction(r) => {
                let mut parts = Vec::new();
                if !r.enumeration.is_empty() {
                    parts.push(r.enumeration.join(" | "));
                }
                if !r.patterns.is_empty() {
                    let list: Vec<String> = r
                        .patterns
                        .iter()
                        .map(|(p, _)| readable_pattern(p))
                        .collect();
                    parts.push(list.join(" | "));
                }
                let bounds = [
                    (r.min_inclusive, "≥"),
                    (r.min_exclusive, ">"),
                    (r.max_inclusive, "≤"),
                    (r.max_exclusive, "<"),
                ];
                for (bound, sign) in bounds {
                    if let Some(b) = bound {
                        parts.push(format!("{sign} {}", PropertyValue::Real(b)));
                    }
                }
                if let Some(l) = r.length {
                    parts.push(format!("{l} characters"));
                }
                match (r.min_length, r.max_length) {
                    (Some(a), Some(b)) => parts.push(format!("{a}-{b} characters")),
                    (Some(a), None) => parts.push(format!("at least {a} characters")),
                    (None, Some(b)) => parts.push(format!("at most {b} characters")),
                    (None, None) => {}
                }
                f.write_str(&parts.join(", "))
            }
        }
    }
}

/// A pattern as shown in results: `[aA]` classes (case-insensitive
/// patterns) folded to the letter and `.*` ends dropped, between slashes.
fn readable_pattern(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '['
            && i + 3 < chars.len()
            && chars[i + 3] == ']'
            && chars[i + 1] != chars[i + 2]
            && chars[i + 1].to_lowercase().eq(chars[i + 2].to_lowercase())
        {
            out.extend(chars[i + 1].to_lowercase());
            i += 4;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    let mut text = out.as_str();
    if let Some(rest) = text.strip_prefix(".*") {
        text = rest;
    }
    if let Some(rest) = text.strip_suffix(".*") {
        text = rest;
    }
    // Drop one pair of parentheses around the whole pattern.
    if text.starts_with('(') && text.ends_with(')') {
        let mut depth = 0;
        let encloses = text.char_indices().all(|(i, c)| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            depth > 0 || i == text.len() - 1
        });
        if encloses {
            text = &text[1..text.len() - 1];
        }
    }
    format!("/{text}/")
}

impl IdsValue {
    /// A pattern that matches any text, e.g. a property set given as `.*`.
    fn is_any(&self) -> bool {
        match self {
            IdsValue::Restriction(r) => {
                r.enumeration.is_empty()
                    && r.patterns.len() == 1
                    && matches!(r.patterns[0].0.as_str(), ".*" | ".+")
            }
            IdsValue::Simple(_) => false,
        }
    }
}

impl Facet {
    /// Short name shown in results, e.g. `Pset_WallCommon.FireRating`.
    pub fn label(&self) -> String {
        let with_value = |base: String, value: &Option<IdsValue>| match value {
            Some(v) => format!("{base} = {v}"),
            None => base,
        };
        match &self.kind {
            FacetKind::Entity(e) => e.label(),
            FacetKind::Attribute { name, value } => with_value(name.to_string(), value),
            FacetKind::Property {
                property_set,
                base_name,
                value,
                ..
            } if property_set.is_any() => with_value(base_name.to_string(), value),
            FacetKind::Property {
                property_set,
                base_name,
                value,
                ..
            } => with_value(format!("{property_set}.{base_name}"), value),
            FacetKind::Classification { system, value } => {
                let mut label = "Classification".to_owned();
                if let Some(s) = system {
                    label.push_str(&format!(" {s}"));
                }
                if let Some(v) = value {
                    label.push_str(&format!(" {v}"));
                }
                label
            }
            FacetKind::Material { value } => match value {
                Some(v) => format!("Material {v}"),
                None => "Material".to_owned(),
            },
            FacetKind::PartOf { relation, entity } => {
                let how = match relation.as_deref() {
                    Some("IFCRELAGGREGATES") => "Part of",
                    Some("IFCRELCONTAINEDINSPATIALSTRUCTURE") => "Contained in",
                    Some("IFCRELASSIGNSTOGROUP") => "In group",
                    Some("IFCRELNESTS") => "Nested in",
                    Some("IFCRELVOIDSELEMENT") => "Voids",
                    Some("IFCRELFILLSELEMENT") => "Fills",
                    _ => "Part of",
                };
                format!("{how} {}", entity.label())
            }
        }
    }
}

impl EntityFacet {
    fn label(&self) -> String {
        match &self.predefined_type {
            Some(p) => format!("{}.{p}", self.name),
            None => self.name.to_string(),
        }
    }
}

// ---- XML reading ----------------------------------------------------------

/// A minimal element tree; IDS files are small.
#[derive(Debug, Default)]
struct Node {
    /// Local name, without namespace prefix.
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn child_text(&self, name: &str) -> String {
        self.child(name)
            .map(|c| c.text.trim().to_owned())
            .unwrap_or_default()
    }
}

fn xml_error(e: impl fmt::Display) -> IdsError {
    IdsError::Xml(e.to_string())
}

fn local_name(qualified: &[u8]) -> String {
    let local = qualified
        .iter()
        .rposition(|b| *b == b':')
        .map_or(qualified, |i| &qualified[i + 1..]);
    String::from_utf8_lossy(local).into_owned()
}

fn start_node(start: &quick_xml::events::BytesStart) -> Result<Node, IdsError> {
    let mut node = Node {
        name: local_name(start.name().as_ref()),
        ..Node::default()
    };
    for attribute in start.attributes() {
        let attribute = attribute.map_err(xml_error)?;
        let key = local_name(attribute.key.as_ref());
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(xml_error)?
            .into_owned();
        node.attributes.push((key, value));
    }
    Ok(node)
}

fn resolve_entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let code = name.strip_prefix('#')?;
            let number = match code.strip_prefix('x').or_else(|| code.strip_prefix('X')) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => code.parse().ok()?,
            };
            char::from_u32(number)
        }
    }
}

fn parse_tree(text: &str) -> Result<Node, IdsError> {
    let mut reader = Reader::from_str(text.trim_start_matches('\u{feff}'));
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(start) => stack.push(start_node(&start)?),
            Event::Empty(start) => {
                let node = start_node(&start)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or_else(|| xml_error("unbalanced end tag"))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::Text(t) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&t.decode().map_err(xml_error)?);
                }
            }
            Event::CData(t) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&t.decode().map_err(xml_error)?);
                }
            }
            Event::GeneralRef(r) => {
                let name = r.decode().map_err(xml_error)?;
                let c = resolve_entity(&name)
                    .ok_or_else(|| xml_error(format!("unknown entity &{name};")))?;
                if let Some(node) = stack.last_mut() {
                    node.text.push(c);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(xml_error("the document ends inside an element"));
    }
    root.ok_or_else(|| xml_error("the document is empty"))
}

fn parse_value(node: &Node) -> Result<IdsValue, IdsError> {
    if let Some(simple) = node.child("simpleValue") {
        return Ok(IdsValue::Simple(simple.text.trim().to_owned()));
    }
    let Some(restriction) = node.child("restriction") else {
        return invalid(format!(
            "<{}> needs a simpleValue or a restriction",
            node.name
        ));
    };
    let mut r = Restriction {
        base: restriction.attribute("base").unwrap_or_default().to_owned(),
        ..Restriction::default()
    };
    let number = |c: &Node| -> Result<f64, IdsError> {
        let text = c.attribute("value").unwrap_or_default().trim();
        text.parse()
            .or_else(|_| invalid(format!("<{}> needs a number, not {text:?}", c.name)))
    };
    let count = |c: &Node| -> Result<usize, IdsError> {
        let text = c.attribute("value").unwrap_or_default().trim();
        text.parse()
            .or_else(|_| invalid(format!("<{}> needs a count, not {text:?}", c.name)))
    };
    for c in &restriction.children {
        let value = || c.attribute("value").unwrap_or_default().to_owned();
        match c.name.as_str() {
            "enumeration" => r.enumeration.push(value()),
            "pattern" => {
                let pattern = value();
                let compiled = compile_pattern(&pattern)?;
                r.patterns.push((pattern, compiled));
            }
            "minInclusive" => r.min_inclusive = Some(number(c)?),
            "maxInclusive" => r.max_inclusive = Some(number(c)?),
            "minExclusive" => r.min_exclusive = Some(number(c)?),
            "maxExclusive" => r.max_exclusive = Some(number(c)?),
            "length" => r.length = Some(count(c)?),
            "minLength" => r.min_length = Some(count(c)?),
            "maxLength" => r.max_length = Some(count(c)?),
            // totalDigits, fractionDigits, whiteSpace, annotations: not checked.
            _ => {}
        }
    }
    Ok(IdsValue::Restriction(r))
}

fn optional_value(node: &Node, name: &str) -> Result<Option<IdsValue>, IdsError> {
    node.child(name).map(parse_value).transpose()
}

fn required_value(node: &Node, name: &str) -> Result<IdsValue, IdsError> {
    match node.child(name) {
        Some(child) => parse_value(child),
        None => invalid(format!("<{}> needs <{name}>", node.name)),
    }
}

fn parse_entity(node: &Node) -> Result<EntityFacet, IdsError> {
    Ok(EntityFacet {
        name: required_value(node, "name")?,
        predefined_type: optional_value(node, "predefinedType")?,
    })
}

fn parse_facet(node: &Node) -> Result<Option<Facet>, IdsError> {
    let kind = match node.name.as_str() {
        "entity" => FacetKind::Entity(parse_entity(node)?),
        "attribute" => FacetKind::Attribute {
            name: required_value(node, "name")?,
            value: optional_value(node, "value")?,
        },
        "property" => FacetKind::Property {
            property_set: required_value(node, "propertySet")?,
            base_name: required_value(node, "baseName")?,
            data_type: node
                .attribute("dataType")
                .map(|d| d.trim().to_ascii_uppercase())
                .filter(|d| !d.is_empty()),
            value: optional_value(node, "value")?,
        },
        "classification" => FacetKind::Classification {
            system: optional_value(node, "system")?,
            value: optional_value(node, "value")?,
        },
        "material" => FacetKind::Material {
            value: optional_value(node, "value")?,
        },
        "partOf" => FacetKind::PartOf {
            relation: node
                .attribute("relation")
                .map(|r| r.trim().to_ascii_uppercase())
                .filter(|r| !r.is_empty()),
            entity: match node.child("entity") {
                Some(e) => parse_entity(e)?,
                None => return invalid("<partOf> needs <entity>"),
            },
        },
        // Annotations and unknown extensions.
        _ => return Ok(None),
    };
    Ok(Some(Facet {
        kind,
        cardinality: Cardinality::parse(node.attribute("cardinality"))?,
        instructions: node
            .attribute("instructions")
            .unwrap_or_default()
            .to_owned(),
        uri: node.attribute("uri").unwrap_or_default().to_owned(),
    }))
}

fn parse_occurs(text: Option<&str>, default: Option<u32>) -> Result<Option<u32>, IdsError> {
    match text.map(str::trim) {
        None => Ok(default),
        Some("unbounded") => Ok(None),
        Some(n) => n
            .parse()
            .map(Some)
            .or_else(|_| invalid(format!("bad occurrence count '{n}'"))),
    }
}

fn parse_specification(node: &Node) -> Result<Specification, IdsError> {
    let name = node.attribute("name").unwrap_or_default().to_owned();
    let mut spec = Specification {
        name: if name.trim().is_empty() {
            "Unnamed specification".to_owned()
        } else {
            name
        },
        identifier: node.attribute("identifier").unwrap_or_default().to_owned(),
        description: node.attribute("description").unwrap_or_default().to_owned(),
        instructions: node
            .attribute("instructions")
            .unwrap_or_default()
            .to_owned(),
        ifc_versions: node
            .attribute("ifcVersion")
            .unwrap_or_default()
            .split_whitespace()
            .map(|v| v.to_ascii_uppercase())
            .collect(),
        min_occurs: 1,
        max_occurs: None,
        ..Specification::default()
    };
    if let Some(applicability) = node.child("applicability") {
        spec.min_occurs = parse_occurs(applicability.attribute("minOccurs"), Some(1))?.unwrap_or(0);
        spec.max_occurs = parse_occurs(applicability.attribute("maxOccurs"), None)?;
        for facet in &applicability.children {
            if let Some(f) = parse_facet(facet)? {
                spec.applicability.push(Facet {
                    cardinality: Cardinality::Required,
                    ..f
                });
            }
        }
    }
    if spec.applicability.is_empty() {
        return invalid(format!(
            "specification '{}' has no applicability",
            spec.name
        ));
    }
    if let Some(requirements) = node.child("requirements") {
        spec.requirements_description = requirements
            .attribute("description")
            .unwrap_or_default()
            .to_owned();
        for facet in &requirements.children {
            if let Some(mut f) = parse_facet(facet)? {
                // The entity facet has no cardinality.
                if matches!(f.kind, FacetKind::Entity(_)) {
                    f.cardinality = Cardinality::Required;
                }
                spec.requirements.push(f);
            }
        }
    }
    Ok(spec)
}

impl Ids {
    /// Reads an IDS 1.0 document.
    pub fn from_xml(text: &str) -> Result<Self, IdsError> {
        let root = parse_tree(text)?;
        if root.name != "ids" {
            return invalid(format!("the root element is <{}>, not <ids>", root.name));
        }
        let info = root.child("info");
        let text_of = |name: &str| info.map(|i| i.child_text(name)).unwrap_or_default();
        let ids = Ids {
            info: IdsInfo {
                title: text_of("title"),
                copyright: text_of("copyright"),
                version: text_of("version"),
                description: text_of("description"),
                author: text_of("author"),
                date: text_of("date"),
                purpose: text_of("purpose"),
                milestone: text_of("milestone"),
            },
            specifications: root
                .child("specifications")
                .map(|s| {
                    s.children_named("specification")
                        .map(parse_specification)
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default(),
        };
        if ids.specifications.is_empty() {
            return invalid("the file has no specifications");
        }
        Ok(ids)
    }

    /// The title, or a fallback for files without one.
    pub fn title(&self) -> &str {
        if self.info.title.trim().is_empty() {
            "IDS"
        } else {
            &self.info.title
        }
    }
}

// ---- XML writing ----------------------------------------------------------

fn escape(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}

fn write_value(out: &mut String, indent: &str, tag: &str, value: &IdsValue) {
    let _ = write!(out, "{indent}<ids:{tag}>");
    match value {
        IdsValue::Simple(s) => {
            let _ = write!(out, "<ids:simpleValue>{}</ids:simpleValue>", escape(s));
        }
        IdsValue::Restriction(r) => {
            let base = if r.base.is_empty() {
                "xs:string"
            } else {
                &r.base
            };
            let _ = writeln!(
                out,
                "\n{indent}  <xs:restriction base=\"{}\">",
                escape(base)
            );
            let facet = |out: &mut String, name: &str, value: String| {
                let _ = writeln!(out, "{indent}    <xs:{name} value=\"{}\"/>", escape(&value));
            };
            for e in &r.enumeration {
                facet(out, "enumeration", e.clone());
            }
            for (p, _) in &r.patterns {
                facet(out, "pattern", p.clone());
            }
            let bounds = [
                ("minInclusive", r.min_inclusive),
                ("maxInclusive", r.max_inclusive),
                ("minExclusive", r.min_exclusive),
                ("maxExclusive", r.max_exclusive),
            ];
            for (name, bound) in bounds {
                if let Some(b) = bound {
                    facet(out, name, b.to_string());
                }
            }
            let lengths = [
                ("length", r.length),
                ("minLength", r.min_length),
                ("maxLength", r.max_length),
            ];
            for (name, length) in lengths {
                if let Some(l) = length {
                    facet(out, name, l.to_string());
                }
            }
            let _ = write!(out, "{indent}  </xs:restriction>\n{indent}");
        }
    }
    let _ = writeln!(out, "</ids:{tag}>");
}

fn write_entity(out: &mut String, indent: &str, entity: &EntityFacet) {
    let inner = format!("{indent}  ");
    let _ = writeln!(out, "{indent}<ids:entity>");
    write_value(out, &inner, "name", &entity.name);
    if let Some(p) = &entity.predefined_type {
        write_value(out, &inner, "predefinedType", p);
    }
    let _ = writeln!(out, "{indent}</ids:entity>");
}

fn write_facet(out: &mut String, indent: &str, facet: &Facet, requirement: bool) {
    let inner = format!("{indent}  ");
    let mut attributes = String::new();
    let mut attribute = |name: &str, value: &str| {
        if !value.is_empty() {
            let _ = write!(attributes, " {name}=\"{}\"", escape(value));
        }
    };
    if let FacetKind::Property {
        data_type: Some(d), ..
    } = &facet.kind
    {
        attribute("dataType", d);
    }
    if let FacetKind::PartOf {
        relation: Some(r), ..
    } = &facet.kind
    {
        attribute("relation", r);
    }
    attribute("uri", &facet.uri);
    if requirement && !matches!(facet.kind, FacetKind::Entity(_)) {
        attribute("cardinality", facet.cardinality.as_str());
    }
    attribute("instructions", &facet.instructions);
    let tag = match &facet.kind {
        FacetKind::Entity(entity) => {
            write_entity(out, indent, entity);
            return;
        }
        FacetKind::Attribute { .. } => "attribute",
        FacetKind::Property { .. } => "property",
        FacetKind::Classification { .. } => "classification",
        FacetKind::Material { .. } => "material",
        FacetKind::PartOf { .. } => "partOf",
    };
    let _ = writeln!(out, "{indent}<ids:{tag}{attributes}>");
    match &facet.kind {
        FacetKind::Entity(_) => unreachable!("written above"),
        FacetKind::Attribute { name, value } => {
            write_value(out, &inner, "name", name);
            if let Some(v) = value {
                write_value(out, &inner, "value", v);
            }
        }
        FacetKind::Property {
            property_set,
            base_name,
            value,
            ..
        } => {
            write_value(out, &inner, "propertySet", property_set);
            write_value(out, &inner, "baseName", base_name);
            if let Some(v) = value {
                write_value(out, &inner, "value", v);
            }
        }
        FacetKind::Classification { system, value } => {
            if let Some(v) = value {
                write_value(out, &inner, "value", v);
            }
            if let Some(s) = system {
                write_value(out, &inner, "system", s);
            }
        }
        FacetKind::Material { value } => {
            if let Some(v) = value {
                write_value(out, &inner, "value", v);
            }
        }
        FacetKind::PartOf { entity, .. } => write_entity(out, &inner, entity),
    }
    let _ = writeln!(out, "{indent}</ids:{tag}>");
}

impl Ids {
    /// The document as IDS 1.0 XML.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let _ = writeln!(
            out,
            "<ids:ids xmlns:ids=\"{IDS_NAMESPACE}\" xmlns:xs=\"{XS_NAMESPACE}\" \
             xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
             xsi:schemaLocation=\"{IDS_NAMESPACE} http://standards.buildingsmart.org/IDS/1.0/ids.xsd\">"
        );
        out.push_str("  <ids:info>\n");
        let info = [
            ("title", self.title()),
            ("copyright", &self.info.copyright),
            ("version", &self.info.version),
            ("description", &self.info.description),
            ("author", &self.info.author),
            ("date", &self.info.date),
            ("purpose", &self.info.purpose),
            ("milestone", &self.info.milestone),
        ];
        for (tag, value) in info {
            if !value.is_empty() {
                let _ = writeln!(out, "    <ids:{tag}>{}</ids:{tag}>", escape(value));
            }
        }
        out.push_str("  </ids:info>\n  <ids:specifications>\n");
        for spec in &self.specifications {
            let mut attributes = format!(" name=\"{}\"", escape(&spec.name));
            let versions = if spec.ifc_versions.is_empty() {
                "IFC2X3 IFC4 IFC4X3_ADD2".to_owned()
            } else {
                spec.ifc_versions.join(" ")
            };
            let _ = write!(attributes, " ifcVersion=\"{versions}\"");
            for (name, value) in [
                ("identifier", &spec.identifier),
                ("description", &spec.description),
                ("instructions", &spec.instructions),
            ] {
                if !value.is_empty() {
                    let _ = write!(attributes, " {name}=\"{}\"", escape(value));
                }
            }
            let _ = writeln!(out, "    <ids:specification{attributes}>");
            let max = spec
                .max_occurs
                .map_or_else(|| "unbounded".to_owned(), |m| m.to_string());
            let _ = writeln!(
                out,
                "      <ids:applicability minOccurs=\"{}\" maxOccurs=\"{max}\">",
                spec.min_occurs
            );
            for facet in &spec.applicability {
                write_facet(&mut out, "        ", facet, false);
            }
            out.push_str("      </ids:applicability>\n");
            if !spec.requirements.is_empty() {
                let description = if spec.requirements_description.is_empty() {
                    String::new()
                } else {
                    format!(
                        " description=\"{}\"",
                        escape(&spec.requirements_description)
                    )
                };
                let _ = writeln!(out, "      <ids:requirements{description}>");
                for facet in &spec.requirements {
                    write_facet(&mut out, "        ", facet, true);
                }
                out.push_str("      </ids:requirements>\n");
            }
            out.push_str("    </ids:specification>\n");
        }
        out.push_str("  </ids:specifications>\n</ids:ids>\n");
        out
    }
}

// ---- Evaluation -------------------------------------------------------------

/// What a model knows that facets need, computed once per model.
pub struct IdsContext<'a> {
    model: &'a IfcModel,
    /// IFC version as IDS writes it: `IFC2X3`, `IFC4` or `IFC4X3_ADD2`.
    version: String,
    /// Spatial node -> (class, parent node).
    spatial: FxHashMap<u32, (&'a str, Option<u32>)>,
}

impl<'a> IdsContext<'a> {
    pub fn new(model: &'a IfcModel) -> Self {
        let mut spatial = FxHashMap::default();
        if let Some(root) = &model.spatial_root {
            fn visit<'a>(
                node: &'a crate::SpatialNode,
                parent: Option<u32>,
                map: &mut FxHashMap<u32, (&'a str, Option<u32>)>,
            ) {
                map.insert(node.express_id, (node.ifc_class.as_str(), parent));
                for child in &node.children {
                    visit(child, Some(node.express_id), map);
                }
            }
            visit(root, None, &mut spatial);
        }
        let schema = model.schema.to_ascii_uppercase();
        let version = if schema.starts_with("IFC4X3") {
            "IFC4X3_ADD2".to_owned()
        } else if schema.starts_with("IFC2X3") {
            "IFC2X3".to_owned()
        } else if schema.starts_with("IFC4") {
            "IFC4".to_owned()
        } else {
            schema
        };
        Self {
            model,
            version,
            spatial,
        }
    }
}

/// How an element relates to one facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// The information is there and matches.
    Match,
    /// The information is there with another value or type.
    Mismatch,
    /// The information is not there.
    Absent,
}

fn has_value(value: &PropertyValue) -> bool {
    match value {
        PropertyValue::Text(s) => !s.trim().is_empty(),
        PropertyValue::List(items) => items.iter().any(has_value),
        PropertyValue::Null | PropertyValue::Logical(None) => false,
        PropertyValue::Range { lower, upper } => has_value(lower) || has_value(upper),
        _ => true,
    }
}

/// Combines the states of alternatives: any match wins, then any mismatch.
fn best(states: impl IntoIterator<Item = State>) -> State {
    let mut result = State::Absent;
    for s in states {
        match s {
            State::Match => return State::Match,
            State::Mismatch => result = State::Mismatch,
            State::Absent => {}
        }
    }
    result
}

/// Combines the states of candidates that must all match: any mismatch
/// fails, and so does a mix of present and absent information.
fn all(states: impl IntoIterator<Item = State>) -> State {
    let (mut matched, mut absent) = (false, false);
    for s in states {
        match s {
            State::Match => matched = true,
            State::Mismatch => return State::Mismatch,
            State::Absent => absent = true,
        }
    }
    match (matched, absent) {
        (true, true) => State::Mismatch,
        (true, false) => State::Match,
        (false, _) => State::Absent,
    }
}

/// Length measures are written in model units; IDS values are in metres.
fn is_length_type(value_type: &str) -> bool {
    matches!(
        value_type.to_ascii_uppercase().as_str(),
        "IFCLENGTHMEASURE"
            | "IFCPOSITIVELENGTHMEASURE"
            | "IFCNONNEGATIVELENGTHMEASURE"
            | "IFCQUANTITYLENGTH"
    )
}

/// The measure type a quantity stands for, e.g. `IfcQuantityArea` -> `IFCAREAMEASURE`.
fn data_type_of(value_type: &str) -> String {
    let upper = value_type.to_ascii_uppercase();
    match upper.as_str() {
        "IFCQUANTITYLENGTH" => "IFCLENGTHMEASURE".to_owned(),
        "IFCQUANTITYAREA" => "IFCAREAMEASURE".to_owned(),
        "IFCQUANTITYVOLUME" => "IFCVOLUMEMEASURE".to_owned(),
        "IFCQUANTITYCOUNT" => "IFCCOUNTMEASURE".to_owned(),
        "IFCQUANTITYWEIGHT" => "IFCMASSMEASURE".to_owned(),
        "IFCQUANTITYTIME" => "IFCTIMEMEASURE".to_owned(),
        "IFCQUANTITYNUMBER" => "IFCNUMERICMEASURE".to_owned(),
        _ => upper,
    }
}

/// Classes of `IfcSystem` and its subtypes, which the model keeps as system names.
const SYSTEM_CLASSES: &[&str] = &[
    "IFCGROUP",
    "IFCSYSTEM",
    "IFCDISTRIBUTIONSYSTEM",
    "IFCDISTRIBUTIONCIRCUIT",
    "IFCBUILDINGSYSTEM",
    "IFCBUILTSYSTEM",
];

impl EntityFacet {
    fn matches_class(&self, class: &str, predefined: Option<&str>) -> bool {
        if !self.name.matches_name(class) {
            return false;
        }
        match &self.predefined_type {
            None => true,
            // Case-sensitive, also for user-defined types.
            Some(p) => predefined.is_some_and(|v| p.matches_text(v)),
        }
    }

    fn matches_element(&self, element: &Element) -> bool {
        // USERDEFINED types carry their name in ObjectType.
        let predefined = match element.predefined_type.as_deref() {
            Some("USERDEFINED") => element.object_type.as_deref().or(Some("USERDEFINED")),
            other => other,
        };
        // IFC2x3 files often write the specific class only on the type.
        self.matches_class(&element.ifc_class, predefined)
            || (element.occurrence_class() != element.ifc_class
                && self.matches_class(element.occurrence_class(), predefined))
    }
}

impl Facet {
    fn state(&self, ctx: &IdsContext, element: &Element) -> State {
        let model = ctx.model;
        match &self.kind {
            FacetKind::Entity(entity) => {
                if entity.matches_element(element) {
                    State::Match
                } else {
                    State::Mismatch
                }
            }
            FacetKind::Attribute { name, value } => {
                let global_id = PropertyValue::Text(element.global_id.clone());
                let candidates = std::iter::once(("GlobalId", &global_id))
                    .chain(element.attributes.iter().map(|(n, v)| (n.as_str(), v)));
                best(
                    candidates
                        .filter(|(n, _)| name.matches_text(n))
                        .map(|(_, v)| {
                            if !has_value(v) {
                                State::Absent
                            } else if value.as_ref().is_none_or(|x| x.matches_value(v, 1.0)) {
                                State::Match
                            } else {
                                State::Mismatch
                            }
                        }),
                )
            }
            FacetKind::Property {
                property_set,
                base_name,
                data_type,
                value,
            } => {
                // Every matching property in every matching set must meet
                // the requirement; a matching set without the property fails.
                let mut seen: Vec<&str> = Vec::new();
                let mut set_states = Vec::new();
                for pset in model.property_sets_of(element) {
                    // The occurrence's set overrides the type's set of the same name.
                    if !property_set.matches_text(&pset.name) || seen.contains(&pset.name.as_str())
                    {
                        continue;
                    }
                    seen.push(pset.name.as_str());
                    let mut states = Vec::new();
                    for p in &pset.properties {
                        if !base_name.matches_text(&p.name) {
                            continue;
                        }
                        if !has_value(&p.value) {
                            states.push(State::Absent);
                            continue;
                        }
                        let value_type = p.value_type.as_deref().unwrap_or_default();
                        if let Some(expected) = data_type
                            && !value_type.is_empty()
                            && data_type_of(value_type) != *expected
                        {
                            states.push(State::Mismatch);
                            continue;
                        }
                        let scale = if is_length_type(value_type) {
                            model.length_unit_scale
                        } else {
                            1.0
                        };
                        states.push(match value {
                            Some(v) if !v.matches_value(&p.value, scale) => State::Mismatch,
                            _ => State::Match,
                        });
                    }
                    set_states.push(all(states));
                }
                all(set_states)
            }
            FacetKind::Classification { system, value } => {
                best(element.classifications.iter().map(|c| {
                    let system_ok = system.as_ref().is_none_or(|s| s.matches_text(&c.system));
                    let value_ok = value.as_ref().is_none_or(|v| v.matches_text(&c.code));
                    if system_ok && value_ok {
                        State::Match
                    } else {
                        State::Mismatch
                    }
                }))
            }
            FacetKind::Material { value } => best(element.materials.iter().map(|m| {
                if value.as_ref().is_none_or(|v| v.matches_text(m)) {
                    State::Match
                } else {
                    State::Mismatch
                }
            })),
            FacetKind::PartOf { relation, entity } => {
                let relation = relation.as_deref();
                let any = relation.is_none();
                let mut states = Vec::new();
                if any || relation == Some("IFCRELAGGREGATES") {
                    let mut top = element;
                    let mut parent = element.parent;
                    let mut depth = 0;
                    while let Some(id) = parent
                        && depth < 32
                    {
                        let Some(whole) = model.element(id) else {
                            break;
                        };
                        states.push(if entity.matches_element(whole) {
                            State::Match
                        } else {
                            State::Mismatch
                        });
                        top = whole;
                        parent = whole.parent;
                        depth += 1;
                    }
                    // Spaces are aggregated into the spatial structure, which
                    // is an aggregation all the way up to the project.
                    if top.ifc_class.eq_ignore_ascii_case("IfcSpace") {
                        let mut node = top.container;
                        while let Some(id) = node
                            && depth < 64
                        {
                            let Some(&(class, up)) = ctx.spatial.get(&id) else {
                                break;
                            };
                            states.push(if entity.matches_class(class, None) {
                                State::Match
                            } else {
                                State::Mismatch
                            });
                            node = up;
                            depth += 1;
                        }
                    }
                }
                if any || relation == Some("IFCRELCONTAINEDINSPATIALSTRUCTURE") {
                    // Parts of an assembly are contained through the assembly.
                    let mut owner = Some(element);
                    let mut container = None;
                    while let Some(e) = owner {
                        if e.container.is_some() {
                            container = e.container;
                            break;
                        }
                        owner = e.parent.and_then(|p| model.element(p));
                    }
                    let mut depth = 0;
                    while let Some(id) = container
                        && depth < 32
                    {
                        let Some(&(class, up)) = ctx.spatial.get(&id) else {
                            // Spaces are elements, not spatial nodes.
                            if let Some(space) = model.element(id) {
                                states.push(if entity.matches_element(space) {
                                    State::Match
                                } else {
                                    State::Mismatch
                                });
                                container = space.container;
                                depth += 1;
                                continue;
                            }
                            break;
                        };
                        // The project decomposes the spatial structure; it
                        // never contains elements.
                        if class.eq_ignore_ascii_case("IfcProject") {
                            break;
                        }
                        states.push(if entity.matches_class(class, None) {
                            State::Match
                        } else {
                            State::Mismatch
                        });
                        container = up;
                        depth += 1;
                    }
                }
                if (any || relation == Some("IFCRELASSIGNSTOGROUP")) && !element.systems.is_empty()
                {
                    // Only system names are known: the class can match, a
                    // predefined type cannot be confirmed.
                    let class_ok = entity.predefined_type.is_none()
                        && SYSTEM_CLASSES.iter().any(|c| entity.name.matches_name(c));
                    states.push(if class_ok {
                        State::Match
                    } else {
                        State::Mismatch
                    });
                }
                // IFCRELNESTS, IFCRELVOIDSELEMENT and IFCRELFILLSELEMENT are
                // not read from the model: such facets are never met.
                best(states)
            }
        }
    }
}

/// Why an element fails a requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Missing,
    WrongValue,
    Prohibited,
}

impl Specification {
    /// The specification is written for models of this IFC version. IDS
    /// treats the version as metadata: other versions are checked too.
    pub fn covers(&self, ctx: &IdsContext) -> bool {
        self.ifc_versions.is_empty() || self.ifc_versions.contains(&ctx.version)
    }

    pub fn applies(&self, ctx: &IdsContext, element: &Element) -> bool {
        self.applicability
            .iter()
            .all(|f| f.state(ctx, element) == State::Match)
    }

    /// The failures per requirement, in requirement order; None where the
    /// requirement is met.
    pub fn check(&self, ctx: &IdsContext, element: &Element) -> Vec<Option<Failure>> {
        // maxOccurs = 0: applicable elements are not allowed at all.
        let prohibited = self.max_occurs == Some(0);
        self.requirements
            .iter()
            .map(|facet| {
                let state = facet.state(ctx, element);
                let cardinality = if prohibited {
                    Cardinality::Prohibited
                } else {
                    facet.cardinality
                };
                match (cardinality, state) {
                    (Cardinality::Required, State::Match) => None,
                    (Cardinality::Required, State::Absent) => Some(Failure::Missing),
                    (Cardinality::Required, State::Mismatch) => Some(Failure::WrongValue),
                    (Cardinality::Optional, State::Mismatch) => Some(Failure::WrongValue),
                    (Cardinality::Optional, _) => None,
                    (Cardinality::Prohibited, State::Match) => Some(Failure::Prohibited),
                    (Cardinality::Prohibited, _) => None,
                }
            })
            .collect()
    }
}

/// Checks the elements of the job's models against an IDS document. Returns
/// the findings, the fill rate per specification and the elements checked.
pub(super) fn check_ids(
    ids: &Ids,
    models: &[CheckModel],
    job: &CheckJob,
    rule: &Rule,
    ruleset_id: &str,
) -> (Vec<Finding>, InformationSummary, usize) {
    let mut summary = InformationSummary {
        standard: ids.title().to_owned(),
        ..InformationSummary::default()
    };
    // Stable keys per specification, unique even when names repeat.
    let keys: Vec<String> = ids
        .specifications
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let base = if s.identifier.is_empty() {
                &s.name
            } else {
                &s.identifier
            };
            let repeated = ids.specifications[..i].iter().any(|o| {
                (if o.identifier.is_empty() {
                    &o.name
                } else {
                    &o.identifier
                }) == base
            });
            if repeated {
                format!("{base}#{i}")
            } else {
                base.clone()
            }
        })
        .collect();
    let mut applicable_count = vec![0usize; ids.specifications.len()];
    let mut covered = vec![false; ids.specifications.len()];
    let mut findings = Vec::new();
    let mut checked = 0;

    for check_model in models.iter().filter(|m| job.models.contains(&m.id)) {
        let model = &check_model.model;
        let ctx = IdsContext::new(model);
        let specs: Vec<usize> = (0..ids.specifications.len()).collect();
        for &s in &specs {
            covered[s] |= ids.specifications[s].covers(&ctx);
        }
        // (element, specification, failures)
        type Outcome = (usize, usize, Vec<Option<Failure>>);
        let results: Vec<Outcome> = model
            .elements
            .par_iter()
            .enumerate()
            .filter(|(e, element)| check_model.includes(*e) && rule.a.matches(model, element))
            .flat_map_iter(|(e, element)| {
                let ctx = &ctx;
                specs.iter().filter_map(move |&s| {
                    let spec = &ids.specifications[s];
                    spec.applies(ctx, element)
                        .then(|| (e, s, spec.check(ctx, element)))
                })
            })
            .collect();
        let mut elements_seen = rustc_hash::FxHashSet::default();

        for (e, s, failures) in results {
            elements_seen.insert(e);
            applicable_count[s] += 1;
            let spec = &ids.specifications[s];
            let coverage = match summary.parts.iter().position(|c| c.part == keys[s]) {
                Some(i) => &mut summary.parts[i],
                None => {
                    summary.parts.push(PartCoverage {
                        part: keys[s].clone(),
                        name: spec.name.clone(),
                        elements: 0,
                        complete: 0,
                        items: spec
                            .requirements
                            .iter()
                            .enumerate()
                            .map(|(i, f)| ItemCoverage {
                                item: i.to_string(),
                                name: requirement_name(f),
                                required: 0,
                                present: 0,
                                found_in: Vec::new(),
                            })
                            .collect(),
                    });
                    summary.parts.last_mut().expect("just pushed")
                }
            };
            coverage.elements += 1;
            // maxOccurs = 0 without requirements: the element may not exist.
            let forbidden = spec.max_occurs == Some(0) && spec.requirements.is_empty();
            let failed = forbidden || failures.iter().any(Option::is_some);
            if !failed {
                coverage.complete += 1;
            }
            for (row, failure) in coverage.items.iter_mut().zip(&failures) {
                row.required += 1;
                if failure.is_none() {
                    row.present += 1;
                }
            }
            if !failed {
                continue;
            }

            let element = &model.elements[e];
            let region =
                finding_region(model, element).transformed(&check_model.transform.as_dmat4());
            let found = FindingElement {
                model: check_model.id,
                express_id: element.express_id,
                global_id: element.global_id.clone(),
                name: element.name.clone(),
                ifc_class: element.ifc_class.clone(),
            };
            let mut details: Vec<String> = spec
                .requirements
                .iter()
                .zip(&failures)
                .filter_map(|(facet, failure)| {
                    let name = requirement_name(facet);
                    match (*failure)? {
                        Failure::Missing => Some(name),
                        Failure::WrongValue => Some(format!("{name} (other value)")),
                        Failure::Prohibited if spec.max_occurs == Some(0) => {
                            Some(format!("{name} (element not allowed)"))
                        }
                        Failure::Prohibited => Some(format!("{name} (not allowed)")),
                    }
                })
                .collect();
            if forbidden {
                details.push("Element not allowed".to_owned());
            }
            findings.push(Finding {
                id: finding_id(
                    ruleset_id,
                    &format!("{}/{}", rule.id, keys[s]),
                    &element.global_id,
                    &element.global_id,
                ),
                kind: FindingKind::MissingInformation,
                a: found.clone(),
                b: found,
                point: region.center(),
                region,
                magnitude: details.len() as f32,
                category: spec.name.clone(),
                details,
            });
        }
        checked += elements_seen.len();
    }

    // Specifications that need applicable elements the models lack.
    for (s, spec) in ids.specifications.iter().enumerate() {
        if !covered[s] {
            summary.notes.push(format!(
                "\"{}\" is written for {}; no model has that IFC version. It was checked anyway.",
                spec.name,
                spec.ifc_versions.join(", ")
            ));
        }
        if spec.min_occurs > 0 && applicable_count[s] < spec.min_occurs as usize {
            summary.notes.push(format!(
                "\"{}\" requires at least {} applicable element(s); found {}.",
                spec.name, spec.min_occurs, applicable_count[s]
            ));
        } else if let Some(max) = spec.max_occurs.filter(|m| *m > 0)
            && applicable_count[s] > max as usize
        {
            summary.notes.push(format!(
                "\"{}\" allows at most {max} applicable element(s); found {}.",
                spec.name, applicable_count[s]
            ));
        }
    }
    summary
        .parts
        .sort_by_key(|c| keys.iter().position(|k| *k == c.part));
    (findings, summary, checked)
}

/// Name of a requirement in results; prohibited ones say so.
fn requirement_name(facet: &Facet) -> String {
    match facet.cardinality {
        Cardinality::Prohibited => format!("No {}", facet.label()),
        _ => facet.label(),
    }
}

/// Where to show a finding: the element, its parts, the assembly it is part
/// of, or as a last resort the whole model.
fn finding_region(model: &IfcModel, element: &Element) -> Aabb {
    let mut bounds = element.bounds;
    for part in &element.parts {
        if let Some(child) = model.element(*part) {
            bounds = bounds.union(&child.bounds);
        }
    }
    let mut parent = element.parent;
    let mut depth = 0;
    while bounds.is_empty()
        && let Some(id) = parent
        && depth < 32
    {
        let Some(whole) = model.element(id) else {
            break;
        };
        bounds = whole.bounds;
        parent = whole.parent;
        depth += 1;
    }
    if !bounds.is_empty() {
        bounds
    } else if !model.bounds.is_empty() {
        model.bounds
    } else {
        Aabb {
            min: glam::Vec3::splat(-0.5),
            max: glam::Vec3::splat(0.5),
        }
    }
}

#[cfg(test)]
mod tests;
