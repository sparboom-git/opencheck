//! Classification systems: the list of codes of a system in one edition, and
//! the check that the code an element carries is in the list and fits the
//! element.
//!
//! An information rule checks that a classification code is *there*. This
//! check is separate and looks at the code itself, when the project has
//! chosen a system:
//!
//! - the code is one of the list's codes at the level elements carry (not a
//!   group heading, not a code reserved for later, not `N/A`);
//! - the type id, when the element has one, begins with the code;
//! - the code fits the element's IFC class, e.g. no pipe code on a wall.
//!
//! Choosing a system also decides where the information rule looks for the
//! classification code and the type id: where that system keeps them, not
//! where other systems do. So the code that counts as there is the code
//! that is checked, and a wall with only a CCS code has no code when the
//! project uses BIMTypeCode. See [`ClassificationSystem::applied_to`].
//!
//! A system is three files in `classifications/`: `<id>.toml` says what the
//! list is and where a model keeps the code, `<id>.txt` is the publisher's
//! list (code, name and level, separated by tabs) and `<id>.expect.toml`
//! holds OpenCheck's own expectations of which IFC classes a code belongs
//! on. The list is kept as published; the expectations are not part of it.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use super::information::{InformationStandard, ItemRole, ModelFacts, Source, validate_sources};
use super::ruleset::{RulesetError, is_a, is_subtype};
use crate::{Element, IfcModel, ModelId};

/// What a list is and where it comes from.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassificationInfo {
    /// Stable identifier of the system in one edition, e.g.
    /// `bimtypecode-2022-dk`.
    pub id: String,
    /// Name of the system, e.g. `BIMTypeCode`.
    pub name: String,
    /// The version, e.g. `2022 DK`.
    pub version: String,
    /// Edition of that version, e.g. `2. udgave, 2023-06-30`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub edition: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    /// Where the list was taken from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Revision of the source, e.g. a commit.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub revision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub licence: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub licence_url: String,
    /// The level of the list's hierarchy whose codes elements carry.
    pub code_level: u8,
    /// Names that mark a code as not in use, e.g. `FRI`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused: Vec<String>,
    /// IFC classes that carry no code and are not checked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_checked: Vec<String>,
}

impl ClassificationInfo {
    /// System and version, e.g. `BIMTypeCode 2022 DK`.
    pub fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }

    /// Edition, publisher and licence on one line, e.g. `2. udgave,
    /// 2023-06-30 · BIM7AA · CC BY-ND 4.0`.
    pub fn provenance(&self) -> String {
        [&self.edition, &self.publisher, &self.licence]
            .into_iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemFile {
    classification: ClassificationInfo,
    /// Where the code is read, as the sources of an information standard.
    #[serde(default)]
    code: Vec<Source>,
    /// Where the type id is read.
    #[serde(default)]
    identifier: Vec<Source>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectFile {
    #[serde(default)]
    not_judged: Vec<String>,
    #[serde(default)]
    sets: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    expect: Vec<ExpectEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectEntry {
    codes: Vec<String>,
    classes: Vec<String>,
}

/// A line of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeEntry {
    pub code: String,
    pub name: String,
    pub level: u8,
}

/// What is wrong with a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CodeProblem {
    /// Not in the list at all, e.g. `570` or `N/A`.
    Unknown,
    /// A group heading, e.g. `57`, where a code of the group is needed.
    Heading,
    /// In the list but reserved, not in use.
    Unused,
    /// The type id does not begin with the code.
    Identifier,
    /// The code does not fit the element's IFC class.
    Element,
}

/// A problem with the code of one element.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeFinding {
    pub kind: CodeProblem,
    /// The element's IFC class, for [`CodeProblem::Element`].
    pub class: String,
    /// Short heading that alike problems share, e.g. `570 · not in the list`.
    pub label: String,
    /// The problem in a sentence.
    pub detail: String,
    /// Help to put it right, e.g. the codes of the group the code seems
    /// to belong to.
    pub hint: Option<String>,
}

/// The code of one element and what is wrong with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Judgement {
    pub code: String,
    /// Name of the code in the list; empty when it is not in the list.
    pub name: String,
    /// Where the code was read, e.g. `BIMTypeCode.BIMTypeCode`.
    pub place: String,
    /// Empty: the code is in order.
    pub problems: Vec<CodeFinding>,
}

/// A classification system in one edition, ready to check against.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassificationSystem {
    pub info: ClassificationInfo,
    /// The list, in the publisher's order.
    pub entries: Vec<CodeEntry>,
    by_code: FxHashMap<String, usize>,
    code_sources: Vec<Source>,
    id_sources: Vec<Source>,
    /// Classes never judged on whether the code fits.
    not_judged: Vec<String>,
    /// Code prefix and the classes codes under it are expected on.
    expectations: Vec<(String, Vec<String>)>,
}

impl ClassificationSystem {
    /// Builds a system from its three files; `expect` may be empty.
    pub fn from_files(system: &str, list: &str, expect: &str) -> Result<Self, RulesetError> {
        let invalid = |m: String| RulesetError::Invalid(m);
        let file: SystemFile = toml::from_str(system)?;
        let info = file.classification;
        if info.id.trim().is_empty() || info.name.trim().is_empty() {
            return Err(invalid("the classification needs an id and a name".into()));
        }
        // At least one place to read the code; the type id is optional.
        validate_sources(&format!("classification '{}', code", info.id), &file.code)?;
        if !file.identifier.is_empty() {
            validate_sources(
                &format!("classification '{}', identifier", info.id),
                &file.identifier,
            )?;
        }

        let mut entries = Vec::new();
        let mut by_code = FxHashMap::default();
        for (n, line) in list.lines().enumerate() {
            let line = line.trim_start_matches('\u{feff}');
            if line.trim().is_empty() {
                continue;
            }
            let mut fields = line.split('\t');
            let code = fields.next().unwrap_or_default().trim();
            let name = fields.next().unwrap_or_default().trim();
            let level = fields.next().unwrap_or_default().trim();
            let Ok(level) = level.parse::<u8>() else {
                return Err(invalid(format!(
                    "classification '{}', line {}: expected code, name and level",
                    info.id,
                    n + 1
                )));
            };
            if code.is_empty() || by_code.insert(code.to_owned(), entries.len()).is_some() {
                return Err(invalid(format!(
                    "classification '{}', line {}: the code is empty or used twice",
                    info.id,
                    n + 1
                )));
            }
            entries.push(CodeEntry {
                code: code.to_owned(),
                name: name.to_owned(),
                level,
            });
        }
        if !entries.iter().any(|e| e.level == info.code_level) {
            return Err(invalid(format!(
                "classification '{}' has no codes at level {}",
                info.id, info.code_level
            )));
        }

        let (not_judged, expectations) = if expect.trim().is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let file: ExpectFile = toml::from_str(expect)?;
            let mut expectations = Vec::new();
            for entry in &file.expect {
                let mut classes = Vec::new();
                for class in &entry.classes {
                    resolve(&file.sets, class, &mut classes, 0).map_err(&invalid)?;
                }
                for code in &entry.codes {
                    if expectations.iter().any(|(c, _)| c == code) {
                        return Err(invalid(format!(
                            "classification '{}': two expectations for '{code}'",
                            info.id
                        )));
                    }
                    expectations.push((code.clone(), classes.clone()));
                }
            }
            (file.not_judged, expectations)
        };

        Ok(Self {
            info,
            entries,
            by_code,
            code_sources: file.code,
            id_sources: file.identifier,
            not_judged,
            expectations,
        })
    }

    /// A line of the list by its code.
    pub fn entry(&self, code: &str) -> Option<&CodeEntry> {
        self.by_code.get(code).map(|&i| &self.entries[i])
    }

    /// The code is reserved, not in use.
    fn is_unused(&self, entry: &CodeEntry) -> bool {
        self.info
            .unused
            .iter()
            .any(|u| u.eq_ignore_ascii_case(entry.name.trim()))
    }

    /// The codes elements may carry: those at the code level that are in use.
    pub fn usable(&self) -> impl Iterator<Item = &CodeEntry> {
        self.entries
            .iter()
            .filter(|e| e.level == self.info.code_level && !self.is_unused(e))
    }

    /// Whether elements of this kind are checked at all.
    pub fn checks(&self, model: &IfcModel, element: &Element) -> bool {
        !self
            .info
            .not_checked
            .iter()
            .any(|c| is_a(model, element, c))
    }

    /// Where this system keeps the code, as sources of an information
    /// standard.
    pub fn code_sources(&self) -> &[Source] {
        &self.code_sources
    }

    /// Where this system keeps the type id; empty when it has none.
    pub fn id_sources(&self) -> &[Source] {
        &self.id_sources
    }

    /// The information standard as it reads with this system chosen: the
    /// item that holds the classification code, and the one that holds the
    /// type id, are read where this system keeps them, and nowhere else.
    /// The standard's own sources for them, which cover several systems,
    /// are replaced, also where a part had sources of its own.
    pub fn applied_to(&self, standard: &InformationStandard) -> InformationStandard {
        let mut out = standard.clone();
        let label = self.info.label();
        let mut replaced = Vec::new();
        for item in &mut out.items {
            let (sources, description) = match item.role {
                Some(ItemRole::Code) => (
                    &self.code_sources,
                    format!("Classification code of {label}."),
                ),
                Some(ItemRole::TypeId) if !self.id_sources.is_empty() => (
                    &self.id_sources,
                    format!("Type id of {label}: the code followed by a running number."),
                ),
                _ => continue,
            };
            item.any.clone_from(sources);
            item.description = description;
            replaced.push(item.id.clone());
        }
        for part in &mut out.parts {
            for id in &replaced {
                part.sources.remove(id);
            }
        }
        out
    }

    /// Where the code (or the type id) is read on an element: where the
    /// standard reads the item with that role, so a project's own choice
    /// under "Where to look" counts; otherwise where the system keeps it.
    fn sources_on<'a>(
        &'a self,
        standard: Option<&'a InformationStandard>,
        role: ItemRole,
        model: &IfcModel,
        element: &Element,
    ) -> &'a [Source] {
        match standard.and_then(|s| Some((s, s.item_with_role(role)?))) {
            Some((standard, item)) => standard.sources_on(model, element, item),
            None if role == ItemRole::Code => &self.code_sources,
            None => &self.id_sources,
        }
    }

    /// The element's code and where it was read; None when it has none.
    /// `standard` is the information standard in force, see
    /// [`ClassificationSystem::judge`].
    pub fn code_on(
        &self,
        standard: Option<&InformationStandard>,
        model: &IfcModel,
        facts: &ModelFacts,
        element: &Element,
    ) -> Option<(String, String)> {
        let sources = self.sources_on(standard, ItemRole::Code, model, element);
        read(sources, model, facts, element)
    }

    /// The classes the code is expected on: those of the longest prefix
    /// with an expectation. None when the code is not judged.
    fn expected(&self, code: &str) -> Option<&[String]> {
        self.expectations
            .iter()
            .filter(|(prefix, _)| code.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, classes)| classes.as_slice())
    }

    /// Whether the code fits the element's class; None when that is not
    /// judged for the element or the code.
    fn fits(&self, model: &IfcModel, element: &Element, code: &str) -> Option<bool> {
        if self.not_judged.iter().any(|c| is_a(model, element, c)) {
            return None;
        }
        let classes = self.expected(code)?;
        let occurrence = element.occurrence_class();
        Some(classes.iter().any(|class| {
            is_a(model, element, class)
                // A generic element (IfcFlowSegment in IFC2x3) may be any of
                // its subtypes; the model does not say which.
                || is_subtype(&model.schema, class, occurrence)
                || is_subtype("IFC4", class, occurrence)
        }))
    }

    /// The element's code and what is wrong with it; None when the element
    /// has no code of this system. With the information standard in force
    /// (see [`ClassificationSystem::applied_to`]) the code is read where
    /// the standard reads it, so what is checked is what counts as there;
    /// without one, where the system keeps it.
    pub fn judge(
        &self,
        standard: Option<&InformationStandard>,
        model: &IfcModel,
        facts: &ModelFacts,
        element: &Element,
    ) -> Option<Judgement> {
        let (code, place) = self.code_on(standard, model, facts, element)?;
        let id_sources = self.sources_on(standard, ItemRole::TypeId, model, element);
        let system = self.info.label();
        let mut problems = Vec::new();
        let mut name = String::new();
        match self.entry(&code) {
            None => problems.push(CodeFinding {
                kind: CodeProblem::Unknown,
                class: String::new(),
                label: format!("{code} · not in the list"),
                detail: format!("{code} is not a code in {system}."),
                hint: self.hint(&code),
            }),
            Some(entry) if self.is_unused(entry) => problems.push(CodeFinding {
                kind: CodeProblem::Unused,
                class: String::new(),
                label: format!("{code} · not in use"),
                detail: format!(
                    "{code} is reserved in {system} ({}) and not in use.",
                    entry.name
                ),
                hint: None,
            }),
            Some(entry) if entry.level != self.info.code_level => {
                name.clone_from(&entry.name);
                problems.push(CodeFinding {
                    kind: CodeProblem::Heading,
                    class: String::new(),
                    label: format!("{code} {} · a group, not a code", entry.name),
                    detail: format!("{code} {} is a group in {system}, not a code.", entry.name),
                    hint: self.hint(&code),
                });
            }
            Some(entry) => {
                name.clone_from(&entry.name);
                if let Some((id, id_place)) = read(id_sources, model, facts, element)
                    && !id.starts_with(&code)
                {
                    problems.push(CodeFinding {
                        kind: CodeProblem::Identifier,
                        class: String::new(),
                        label: format!("{code} · the type id begins differently"),
                        detail: format!(
                            "The type id {id} ({id_place}) does not begin with the code {code}."
                        ),
                        hint: None,
                    });
                }
                if self.fits(model, element, &code) == Some(false) {
                    let class = element.occurrence_class().to_owned();
                    let short = class.strip_prefix("Ifc").unwrap_or(&class).to_owned();
                    problems.push(CodeFinding {
                        kind: CodeProblem::Element,
                        label: format!("{code} {} · on {short}", entry.name),
                        detail: format!(
                            "{code} {} is not a code for an element of the class {class}.",
                            entry.name
                        ),
                        hint: None,
                        class,
                    });
                }
            }
        }
        Some(Judgement {
            code,
            name,
            place,
            problems,
        })
    }

    /// Help for a code that is not usable: the group it seems to belong to
    /// and that group's codes, e.g. `Group 57 Ventilation has the codes
    /// 571, 572, ...`.
    fn hint(&self, code: &str) -> Option<String> {
        let group = (1..=code.len())
            .rev()
            .filter(|&n| code.is_char_boundary(n))
            .filter_map(|n| self.entry(&code[..n]))
            .find(|e| e.level < self.info.code_level)?;
        let codes: Vec<&str> = self
            .usable()
            .filter(|e| e.code.starts_with(&group.code))
            .map(|e| e.code.as_str())
            .collect();
        if codes.is_empty() {
            return Some(format!(
                "Group {} {} has no codes in use.",
                group.code, group.name
            ));
        }
        let list = if codes.len() > 12 {
            format!("{} to {}", codes[0], codes[codes.len() - 1])
        } else {
            codes.join(", ")
        };
        Some(format!(
            "Group {} {} has the codes {list}.",
            group.code, group.name
        ))
    }
}

/// Replaces `@name` by the classes of the set, sets in sets included.
fn resolve(
    sets: &BTreeMap<String, Vec<String>>,
    class: &str,
    out: &mut Vec<String>,
    depth: usize,
) -> Result<(), String> {
    let Some(name) = class.strip_prefix('@') else {
        if !out.iter().any(|c| c.eq_ignore_ascii_case(class)) {
            out.push(class.to_owned());
        }
        return Ok(());
    };
    let set = sets
        .get(name)
        .ok_or_else(|| format!("expectations: unknown set '@{name}'"))?;
    if depth > 8 {
        return Err(format!("expectations: set '@{name}' contains itself"));
    }
    for class in set {
        resolve(sets, class, out, depth + 1)?;
    }
    Ok(())
}

/// The first text the sources find on the element, and where. Sources
/// that hold no text (storey, geometry) are passed over.
fn read(
    sources: &[Source],
    model: &IfcModel,
    facts: &ModelFacts,
    element: &Element,
) -> Option<(String, String)> {
    sources.iter().find_map(|source| {
        source
            .value(model, facts, element)
            .filter(|(value, _)| !value.is_empty())
    })
}

/// How often one problem occurs in a model.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeProblemCount {
    pub kind: CodeProblem,
    pub code: String,
    /// Name of the code in the list; empty when it is not in the list.
    pub name: String,
    /// The element's IFC class, for [`CodeProblem::Element`].
    pub class: String,
    /// As [`CodeFinding::label`].
    pub label: String,
    pub count: usize,
}

/// The codes of one model.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassificationModel {
    pub model: ModelId,
    pub name: String,
    /// Elements checked.
    pub elements: usize,
    /// Of them, elements with a code of the system.
    pub coded: usize,
    /// Of those, elements whose code is in order.
    pub valid: usize,
    /// What is wrong, most frequent first.
    pub problems: Vec<CodeProblemCount>,
    /// Where the codes were read and on how many elements.
    pub found_in: Vec<(String, usize)>,
}

impl ClassificationModel {
    /// Elements with a code that is not in order.
    pub fn wrong(&self) -> usize {
        self.coded - self.valid
    }

    /// Elements without a code of the system.
    pub fn without(&self) -> usize {
        self.elements - self.coded
    }

    /// Share of the codes that are in order; 1 when the model has none.
    pub fn rate(&self) -> f64 {
        if self.coded == 0 {
            1.0
        } else {
            self.valid as f64 / self.coded as f64
        }
    }

    /// Counts one element's judgement.
    pub fn count(&mut self, judgement: Option<&Judgement>) {
        self.elements += 1;
        let Some(judgement) = judgement else {
            return;
        };
        self.coded += 1;
        match self
            .found_in
            .iter_mut()
            .find(|(p, _)| *p == judgement.place)
        {
            Some((_, n)) => *n += 1,
            None => self.found_in.push((judgement.place.clone(), 1)),
        }
        if judgement.problems.is_empty() {
            self.valid += 1;
        }
        for problem in &judgement.problems {
            match self.problems.iter_mut().find(|p| p.label == problem.label) {
                Some(row) => row.count += 1,
                None => self.problems.push(CodeProblemCount {
                    kind: problem.kind,
                    code: judgement.code.clone(),
                    name: judgement.name.clone(),
                    class: problem.class.clone(),
                    label: problem.label.clone(),
                    count: 1,
                }),
            }
        }
    }

    /// Orders the rows for showing: most frequent first.
    pub fn tidy(&mut self) {
        self.problems
            .sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
        self.found_in
            .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    }
}

/// The classification check of an information rule: the system used and
/// the codes per model.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClassificationSummary {
    pub info: ClassificationInfo,
    /// Per model, in the order checked.
    pub models: Vec<ClassificationModel>,
}

impl ClassificationSummary {
    pub fn elements(&self) -> usize {
        self.models.iter().map(|m| m.elements).sum()
    }

    pub fn coded(&self) -> usize {
        self.models.iter().map(|m| m.coded).sum()
    }

    pub fn wrong(&self) -> usize {
        self.models.iter().map(ClassificationModel::wrong).sum()
    }
}

/// A classification system that ships with OpenCheck.
pub struct BuiltinClassification {
    pub id: &'static str,
    pub system: &'static str,
    pub list: &'static str,
    pub expect: &'static str,
}

pub const BUILTIN_CLASSIFICATIONS: &[BuiltinClassification] = &[BuiltinClassification {
    id: "bimtypecode-2022-dk",
    system: include_str!("../../classifications/bimtypecode-2022-dk.toml"),
    list: include_str!("../../classifications/bimtypecode-2022-dk.txt"),
    expect: include_str!("../../classifications/bimtypecode-2022-dk.expect.toml"),
}];

/// The built-in systems, parsed once, in the order they are offered.
pub fn builtin_classifications() -> &'static [Arc<ClassificationSystem>] {
    static PARSED: LazyLock<Vec<Arc<ClassificationSystem>>> = LazyLock::new(|| {
        BUILTIN_CLASSIFICATIONS
            .iter()
            .map(|b| {
                Arc::new(
                    ClassificationSystem::from_files(b.system, b.list, b.expect)
                        .expect("built-in classifications parse"),
                )
            })
            .collect()
    });
    &PARSED
}

/// A built-in system by id.
pub fn builtin_classification(id: &str) -> Option<Arc<ClassificationSystem>> {
    builtin_classifications()
        .iter()
        .find(|s| s.info.id == id)
        .cloned()
}

#[cfg(test)]
mod tests {
    use glam::{DMat4, Mat4, Vec3};

    use super::*;
    use crate::check::{CheckJob, CheckModel, FindingKind, Ruleset, run_check};
    use crate::{
        Aabb, ClassificationRef, Discipline, ElementMesh, LoadStats, Property, PropertySet,
        PropertySetKind, PropertyValue,
    };

    fn system() -> Arc<ClassificationSystem> {
        builtin_classification("bimtypecode-2022-dk").expect("built in")
    }

    /// An element with a type code in a property set of its own (express
    /// id + 1000) and, when given, a type id.
    fn coded(id: u32, class: &str, code: &str, type_id: Option<&str>) -> (Element, PropertySet) {
        let text = |name: &str, value: &str| Property {
            name: name.to_owned(),
            value: PropertyValue::Text(value.to_owned()),
            value_type: Some("IfcIdentifier".to_owned()),
        };
        let mut properties = vec![text("BIMTypeCode", code)];
        if let Some(type_id) = type_id {
            properties.push(text("BIMTypeID", type_id));
        }
        let mut element = plain(id, class);
        element.property_sets = vec![id + 1000];
        let pset = PropertySet {
            express_id: id + 1000,
            name: "BIMTypeCode".to_owned(),
            kind: PropertySetKind::Properties,
            properties,
        };
        (element, pset)
    }

    /// An element without a code. It has a part, so it counts as checked
    /// without a mesh.
    fn plain(id: u32, class: &str) -> Element {
        Element {
            express_id: id,
            global_id: format!("guid{id}"),
            ifc_class: class.to_owned(),
            name: format!("{class} {id}"),
            description: None,
            object_type: None,
            tag: None,
            predefined_type: None,
            container: None,
            parent: None,
            parts: vec![1],
            type_object: None,
            type_class: None,
            attributes: Vec::new(),
            classifications: Vec::new(),
            materials: Vec::new(),
            systems: Vec::new(),
            property_sets: Vec::new(),
            meshes: Vec::new(),
            bounds: Aabb {
                min: Vec3::ZERO,
                max: Vec3::ONE,
            },
        }
    }

    fn model(schema: &str, parts: Vec<(Element, PropertySet)>, extra: Vec<Element>) -> IfcModel {
        let mut elements = Vec::new();
        let mut property_sets = FxHashMap::default();
        for (element, pset) in parts {
            elements.push(element);
            property_sets.insert(pset.express_id, pset);
        }
        elements.extend(extra);
        IfcModel::new(
            None,
            "test".to_owned(),
            Discipline::Architectural,
            schema.to_owned(),
            None,
            None,
            0,
            0.001,
            None,
            elements,
            property_sets,
            Vec::<ElementMesh>::new(),
            DMat4::IDENTITY,
            LoadStats::default(),
        )
    }

    /// The kinds of problem the system finds on element `index`.
    fn problems(model: &IfcModel, index: usize) -> Vec<CodeProblem> {
        system()
            .judge(None, model, &ModelFacts::new(model), &model.elements[index])
            .expect("the element has a code")
            .problems
            .iter()
            .map(|p| p.kind)
            .collect()
    }

    #[test]
    fn judges_the_code_against_the_list() {
        let model = model(
            "IFC4",
            vec![
                coded(10, "IfcWall", "224", Some("224019")),
                coded(11, "IfcWall", "570", None),
                coded(12, "IfcWall", "22", None),
                coded(13, "IfcWall", "011", None),
                coded(14, "IfcWall", "N/A", None),
                coded(15, "IfcWall", "224", Some("225019")),
            ],
            Vec::new(),
        );
        let system = system();
        let facts = ModelFacts::new(&model);
        let good = system
            .judge(None, &model, &facts, &model.elements[0])
            .unwrap();
        assert_eq!(good.code, "224");
        assert_eq!(good.name, "Indvendige skeletkonstruerede vægge");
        assert_eq!(good.place, "BIMTypeCode.BIMTypeCode");
        assert!(good.problems.is_empty());

        assert_eq!(problems(&model, 1), vec![CodeProblem::Unknown]);
        let unknown = system
            .judge(None, &model, &facts, &model.elements[1])
            .unwrap();
        assert_eq!(unknown.problems[0].label, "570 · not in the list");
        assert_eq!(
            unknown.problems[0].detail,
            "570 is not a code in BIMTypeCode 2022 DK."
        );
        assert!(
            unknown.problems[0]
                .hint
                .as_ref()
                .is_some_and(|h| h.starts_with("Group 57 Ventilation has the codes 571,"))
        );
        // A group heading, a reserved code and a text that is no code.
        assert_eq!(problems(&model, 2), vec![CodeProblem::Heading]);
        assert_eq!(problems(&model, 3), vec![CodeProblem::Unused]);
        assert_eq!(problems(&model, 4), vec![CodeProblem::Unknown]);
        // The type id must begin with the code.
        assert_eq!(problems(&model, 5), vec![CodeProblem::Identifier]);
    }

    #[test]
    fn judges_whether_the_code_fits_the_element() {
        let mut generic = coded(23, "IfcFlowSegment", "571", None);
        generic.0.type_class = None;
        let mut typed = coded(24, "IfcFlowSegment", "571", None);
        typed.0.type_class = Some("IfcPipeSegmentType".to_owned());
        let model = model(
            "IFC2X3",
            vec![
                // A pipe code on a wall and a wall code on a duct.
                coded(20, "IfcWallStandardCase", "591", None),
                coded(21, "IfcDuctSegment", "224", None),
                // A proxy is never judged; supports may be beams.
                coded(22, "IfcBuildingElementProxy", "591", None),
                generic,
                typed,
                coded(25, "IfcBeam", "599", None),
                coded(26, "IfcCovering", "598", None),
            ],
            Vec::new(),
        );
        assert_eq!(problems(&model, 0), vec![CodeProblem::Element]);
        let wall = system()
            .judge(None, &model, &ModelFacts::new(&model), &model.elements[0])
            .unwrap();
        assert_eq!(
            wall.problems[0].label,
            "591 Metalrør i bygninger · on WallStandardCase"
        );
        assert_eq!(problems(&model, 1), vec![CodeProblem::Element]);
        assert!(problems(&model, 2).is_empty());
        // A generic flow segment may be a duct; one typed as a pipe is not.
        assert!(problems(&model, 3).is_empty());
        assert_eq!(problems(&model, 4), vec![CodeProblem::Element]);
        assert!(problems(&model, 5).is_empty());
        assert!(problems(&model, 6).is_empty());
    }

    #[test]
    fn reads_the_code_from_a_classification_reference_of_the_system() {
        let reference = |system: &str, code: &str| ClassificationRef {
            system: system.to_owned(),
            code: code.to_owned(),
            name: String::new(),
        };
        let mut revit = plain(30, "IfcWall");
        revit.classifications = vec![reference("Uniformat", "213")];
        let mut other = plain(31, "IfcWall");
        other.classifications = vec![reference("CCS", "[L]ULM")];
        let mut both = plain(32, "IfcWall");
        both.classifications = vec![reference("CCS", "[L]ULM"), reference("BIM7AA", "224")];
        let model = model("IFC4", Vec::new(), vec![revit, other, both]);
        let system = system();
        let facts = ModelFacts::new(&model);
        let code = |i: usize| system.code_on(None, &model, &facts, &model.elements[i]);
        assert_eq!(
            code(0),
            Some(("213".to_owned(), "Classification Uniformat".to_owned()))
        );
        // A code of another system is no code of this one.
        assert_eq!(code(1), None);
        assert_eq!(code(2).unwrap().0, "224");
    }

    #[test]
    fn the_chosen_system_decides_where_the_standard_reads_the_code() {
        let base = crate::check::builtin_standard("opencheck.information.lod-dk").unwrap();
        let system = system();
        let narrowed = system.applied_to(base);
        let code = narrowed.item_with_role(ItemRole::Code).unwrap();
        assert_eq!(code.id, "classification-code");
        assert_eq!(code.any, system.code_sources());
        assert_eq!(
            code.description,
            "Classification code of BIMTypeCode 2022 DK."
        );
        let type_id = narrowed.item_with_role(ItemRole::TypeId).unwrap();
        assert_eq!(type_id.any, system.id_sources());
        // The standard's own sources cover several systems; they are gone.
        let names = |item: &crate::check::InformationItem| -> String {
            item.any
                .iter()
                .map(Source::label)
                .collect::<Vec<_>>()
                .join("; ")
        };
        assert!(names(base.item("classification-code").unwrap()).contains("CCS"));
        assert!(!names(code).contains("CCS"));
        assert!(!names(type_id).contains("CCS"));
        // Nothing else changes.
        for (before, after) in base.items.iter().zip(&narrowed.items) {
            if before.role.is_none() {
                assert_eq!(before, after);
            }
        }
        assert_eq!(base.parts.len(), narrowed.parts.len());
    }

    #[test]
    fn a_code_of_another_system_is_no_code_when_a_system_is_chosen() {
        let ruleset = Ruleset::from_toml(include_str!("../../rulesets/information-lod-dk.toml"))
            .expect("built-in ruleset");
        // A wall with a CCS code only, in a property and as a reference;
        // and a wall with both a CCS code and a type code.
        let text = |name: &str, value: &str| Property {
            name: name.to_owned(),
            value: PropertyValue::Text(value.to_owned()),
            value_type: Some("IfcIdentifier".to_owned()),
        };
        let ccs = ClassificationRef {
            system: "CCS".to_owned(),
            code: "[L]ULM".to_owned(),
            name: "Vægplade".to_owned(),
        };
        let mut only_ccs = plain(50, "IfcWall");
        only_ccs.classifications = vec![ccs.clone()];
        only_ccs.property_sets = vec![1050];
        let ccs_set = PropertySet {
            express_id: 1050,
            name: "NIRAS CCS".to_owned(),
            kind: PropertySetKind::Properties,
            properties: vec![
                text("CCSClassCode", "[L]ULM"),
                text("CCSTypeID", "[L]%ULM630"),
            ],
        };
        let (mut both, mut both_set) = coded(51, "IfcWall", "211", Some("211630"));
        both.classifications = vec![ccs];
        both_set.properties.push(text("CCSClassCode", "[L]ULM"));
        let model = model(
            "IFC4",
            vec![(only_ccs, ccs_set), (both, both_set)],
            Vec::new(),
        );
        let models = vec![CheckModel {
            id: ModelId(1),
            model: Arc::new(model),
            transform: Mat4::IDENTITY,
            mask: None,
        }];
        let run = |classification| {
            let job = CheckJob {
                ruleset: ruleset.clone(),
                models: vec![ModelId(1)],
                levels: Vec::new(),
                ids: Vec::new(),
                standards: Vec::new(),
                classification,
            };
            run_check(&models, &[job], &|_| {})
                .rulesets
                .remove(0)
                .rules
                .remove(0)
        };
        // How many of the walls have the item, and where it was found.
        let item = |rule: &crate::check::RuleResult, id: &str| {
            let summary = rule.information.as_ref().unwrap();
            let wall = summary.parts.iter().find(|p| p.part == "wall").unwrap();
            let row = wall.items.iter().find(|i| i.item == id).unwrap();
            (row.present, row.required, row.found_in.clone())
        };

        // Without a system, any classification counts.
        let any = run(None);
        assert_eq!(item(&any, "classification-code").0, 2);
        assert_eq!(item(&any, "type-id").0, 2);

        // With BIMTypeCode chosen, the CCS code is no code: the wall lacks
        // the item, and the wall with both is found on its type code.
        let chosen = run(Some(system()));
        let (present, required, found_in) = item(&chosen, "classification-code");
        assert_eq!((present, required), (1, 2));
        assert_eq!(found_in, vec![("BIMTypeCode.BIMTypeCode".to_owned(), 1)]);
        assert_eq!(item(&chosen, "type-id").0, 1);
        let missing = chosen
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::MissingInformation && f.a.express_id == 50)
            .unwrap();
        assert!(missing.details.contains(&"Klassifikationskode".to_owned()));
        assert!(missing.details.contains(&"Type (-kode/-ID)".to_owned()));
        // The code check and the fill rate agree: one wall has a code, and
        // it is in order.
        let codes = chosen
            .information
            .as_ref()
            .unwrap()
            .classification
            .as_ref()
            .unwrap();
        let row = &codes.models[0];
        assert_eq!((row.elements, row.coded, row.valid), (2, 1, 1));
        assert!(
            chosen
                .findings
                .iter()
                .all(|f| f.kind != FindingKind::Classification)
        );
    }

    #[test]
    fn the_code_is_checked_where_the_project_reads_it() {
        let ruleset = Ruleset::from_toml(include_str!("../../rulesets/information-lod-dk.toml"))
            .expect("built-in ruleset");
        // The project keeps its type code in a property of its own.
        let mut wall = plain(60, "IfcWall");
        wall.property_sets = vec![1060];
        let set = PropertySet {
            express_id: 1060,
            name: "Firma".to_owned(),
            kind: PropertySetKind::Properties,
            properties: vec![Property {
                name: "Typekode".to_owned(),
                value: PropertyValue::Integer(570),
                value_type: Some("IfcInteger".to_owned()),
            }],
        };
        let model = model("IFC4", vec![(wall, set)], Vec::new());
        let models = vec![CheckModel {
            id: ModelId(1),
            model: Arc::new(model),
            transform: Mat4::IDENTITY,
            mask: None,
        }];
        let system = system();
        let base = crate::check::builtin_standard("opencheck.information.lod-dk").unwrap();
        let mut overrides = crate::check::StandardOverrides::new("opencheck.information.lod-dk");
        overrides.items.insert(
            "classification-code".to_owned(),
            vec![Source {
                property: Some("^Typekode$".to_owned()),
                ..Source::default()
            }],
        );
        let (adapted, skipped) = overrides.apply(&system.applied_to(base));
        assert!(skipped.is_empty());
        let job = CheckJob {
            ruleset,
            models: vec![ModelId(1)],
            levels: Vec::new(),
            ids: Vec::new(),
            standards: vec![Arc::new(adapted)],
            classification: Some(system),
        };
        let report = run_check(&models, &[job], &|_| {});
        let rule = &report.rulesets[0].rules[0];
        let code = rule
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::Classification)
            .expect("the project's own property is checked");
        assert_eq!(code.category, "570 · not in the list");
        assert_eq!(
            code.details.last().unwrap(),
            "The code is read from Firma.Typekode."
        );
    }

    #[test]
    fn the_check_runs_with_the_information_rule_and_keeps_its_findings_apart() {
        let ruleset = Ruleset::from_toml(include_str!("../../rulesets/information-lod-dk.toml"))
            .expect("built-in ruleset");
        let mut space = plain(43, "IfcSpace");
        space.name = "Room".to_owned();
        let model = model(
            "IFC4",
            vec![
                coded(40, "IfcWall", "224", None),
                coded(41, "IfcWall", "570", None),
                coded(42, "IfcWall", "591", None),
            ],
            vec![space, plain(44, "IfcSlab")],
        );
        let models = vec![CheckModel {
            id: ModelId(1),
            model: Arc::new(model),
            transform: Mat4::IDENTITY,
            mask: None,
        }];
        let job = |classification| CheckJob {
            ruleset: ruleset.clone(),
            models: vec![ModelId(1)],
            levels: Vec::new(),
            ids: Vec::new(),
            standards: Vec::new(),
            classification,
        };

        // Without a system the rule gives what it gave before.
        let before = run_check(&models, &[job(None)], &|_| {});
        let before = &before.rulesets[0].rules[0];
        assert!(
            before
                .information
                .as_ref()
                .unwrap()
                .classification
                .is_none()
        );
        assert!(
            before
                .findings
                .iter()
                .all(|f| f.kind == FindingKind::MissingInformation)
        );

        let report = run_check(&models, &[job(Some(system()))], &|_| {});
        let rule = &report.rulesets[0].rules[0];
        let (codes, missing): (Vec<_>, Vec<_>) = rule
            .findings
            .iter()
            .partition(|f| f.kind == FindingKind::Classification);
        // The missing information findings are the same, with the same ids.
        let ids = |list: &[&crate::check::Finding]| -> Vec<String> {
            list.iter().map(|f| f.id.clone()).collect()
        };
        assert_eq!(
            ids(&missing),
            ids(&before.findings.iter().collect::<Vec<_>>())
        );
        assert!(missing.iter().all(|m| codes.iter().all(|c| c.id != m.id)));

        // The space is not checked; the slab has no code and is counted,
        // not reported.
        let summary = rule
            .information
            .as_ref()
            .unwrap()
            .classification
            .as_ref()
            .unwrap();
        let row = &summary.models[0];
        assert_eq!((row.elements, row.coded, row.valid), (4, 3, 1));
        assert_eq!((row.wrong(), row.without()), (2, 1));
        assert_eq!(codes.len(), 2);
        let unknown = codes.iter().find(|f| f.a.express_id == 41).unwrap();
        assert_eq!(unknown.category, "570 · not in the list");
        // The problem, the help and where the code was read, a line each.
        assert_eq!(
            unknown.details,
            vec![
                "570 is not a code in BIMTypeCode 2022 DK.",
                "Group 57 Ventilation has the codes 571, 572, 573, 574, 575, 576, 577, 578, 579.",
                "The code is read from BIMTypeCode.BIMTypeCode.",
            ]
        );
        let pipe = codes.iter().find(|f| f.a.express_id == 42).unwrap();
        assert_eq!(pipe.category, "591 Metalrør i bygninger · on Wall");
        assert_eq!(row.problems.len(), 2);
        assert_eq!(
            row.found_in,
            vec![("BIMTypeCode.BIMTypeCode".to_owned(), 3)]
        );
    }

    #[test]
    fn the_built_in_list_is_the_published_one() {
        let system = system();
        assert_eq!(system.info.label(), "BIMTypeCode 2022 DK");
        assert_eq!(
            system.info.provenance(),
            "2. udgave, 2023-06-30 · BIM7AA · CC BY-ND 4.0"
        );
        // 10 main groups, 75 groups and 330 codes, of which 34 are reserved.
        let level = |n| system.entries.iter().filter(|e| e.level == n).count();
        assert_eq!((level(1), level(2), level(3)), (10, 75, 330));
        assert_eq!(system.usable().count(), 330 - 34);
        assert_eq!(
            system.entry("224").unwrap().name,
            "Indvendige skeletkonstruerede vægge"
        );
        assert_eq!(system.entry("57").unwrap().name, "Ventilation");
        assert!(system.entry("570").is_none());
        // Reserved codes are in the list but not usable.
        assert!(system.entry("011").is_some());
        assert!(!system.usable().any(|e| e.code == "011"));
    }

    #[test]
    fn hints_name_the_group_and_its_codes() {
        let system = system();
        assert_eq!(
            system.hint("570").unwrap(),
            "Group 57 Ventilation has the codes 571, 572, 573, 574, 575, 576, 577, 578, 579."
        );
        assert_eq!(
            system.hint("012").unwrap(),
            "Group 01 Generiske objekter ARK has no codes in use."
        );
        assert_eq!(system.hint("N/A"), None);
    }

    #[test]
    fn the_longest_prefix_decides_what_is_expected() {
        let system = system();
        let has = |code: &str, class: &str| {
            system
                .expected(code)
                .is_some_and(|c| c.iter().any(|x| x == class))
        };
        // 59 is pipes; 599 (supports) also takes building elements.
        assert!(has("591", "IfcPipeSegment"));
        assert!(!has("591", "IfcDuctSegment"));
        assert!(!has("591", "IfcBuildingElement"));
        assert!(has("599", "IfcBuildingElement"));
        assert!(has("571", "IfcDuctSegment"));
        assert!(!has("571", "IfcPipeSegment"));
        // Sets are resolved, sets in sets included.
        assert!(has("224", "IfcElementAssembly"));
        // Reserved groups have no expectation.
        assert!(system.expected("011").is_none());
    }

    #[test]
    fn rejects_broken_files() {
        let info = "[classification]\nid = \"x\"\nname = \"X\"\nversion = \"1\"\ncode_level = 2\n";
        let source = "[[code]]\nproperty = \"^Code$\"\n";
        let ok = format!("{info}{source}");
        assert!(ClassificationSystem::from_files(&ok, "1\tA\t1\n11\tB\t2\n", "").is_ok());
        // No source, no codes at the code level, a code twice, no level.
        assert!(ClassificationSystem::from_files(info, "11\tB\t2\n", "").is_err());
        assert!(ClassificationSystem::from_files(&ok, "1\tA\t1\n", "").is_err());
        assert!(ClassificationSystem::from_files(&ok, "11\tA\t2\n11\tB\t2\n", "").is_err());
        assert!(ClassificationSystem::from_files(&ok, "11\tA\n", "").is_err());
        // An unknown set and two expectations for one prefix.
        let list = "11\tB\t2\n";
        let unknown = "[[expect]]\ncodes = [\"1\"]\nclasses = [\"@nowhere\"]\n";
        assert!(ClassificationSystem::from_files(&ok, list, unknown).is_err());
        let twice = "[[expect]]\ncodes = [\"1\", \"1\"]\nclasses = [\"IfcWall\"]\n";
        assert!(ClassificationSystem::from_files(&ok, list, twice).is_err());
    }
}
