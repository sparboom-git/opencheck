//! Information requirements: which information each kind of building part
//! must carry at a level of development (LOD), and the check of models
//! against them.
//!
//! A standard is a TOML file with three parts:
//!
//! - `[standard]`: id, name and the levels it defines, lowest first.
//! - `[[item]]`: a piece of information, e.g. "classification code", with the
//!   places it may be found in (`any`). Projects store the same information in
//!   different places, so an item is present when any one source has a value.
//! - `[[part]]`: a kind of building part, the elements it applies to and the
//!   items it needs per level. An element belongs to the first part that
//!   applies to it.
//!
//! The format is close to buildingSMART IDS (applicability and requirements),
//! with alternatives per requirement added.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use super::ruleset::{RulesetError, Selector};
use crate::{Element, IfcModel, PropertyValue};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardInfo {
    /// Stable identifier, e.g. `opencheck.information.lod-dk`.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    /// Level names, lowest first, e.g. `["200", "300", "325"]`.
    pub levels: Vec<String>,
    /// Level used when the project does not choose one.
    pub default_level: String,
}

fn default_version() -> String {
    "1".to_owned()
}

/// One place an item of information may be found. Exactly one field is set
/// (`pset` only together with `property`). Patterns are regular expressions,
/// matched anywhere in the name and ignoring case; anchor them with `^...$`
/// for exact names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Source {
    /// A property or quantity whose name matches, with a value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property: Option<String>,
    /// Limits `property` to property sets whose name matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pset: Option<String>,
    /// A direct IFC attribute (`Name`, `ObjectType`, `LongName`, ...) with a value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,
    /// The element has a type object with a name.
    #[serde(skip_serializing_if = "is_false")]
    pub type_name: bool,
    /// A classification reference on the element or its type.
    #[serde(skip_serializing_if = "is_false")]
    pub classification: bool,
    /// A material on the element or its type.
    #[serde(skip_serializing_if = "is_false")]
    pub material: bool,
    /// The element is assigned to a system.
    #[serde(skip_serializing_if = "is_false")]
    pub system: bool,
    /// The element is contained in a building storey.
    #[serde(skip_serializing_if = "is_false")]
    pub storey: bool,
    /// The information follows from the element's geometry and placement
    /// (levels, positions); present whenever the element has geometry.
    #[serde(skip_serializing_if = "is_false")]
    pub geometry: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Source {
    fn kinds(&self) -> usize {
        [
            self.property.is_some(),
            self.attribute.is_some(),
            self.type_name,
            self.classification,
            self.material,
            self.system,
            self.storey,
            self.geometry,
        ]
        .into_iter()
        .filter(|k| *k)
        .count()
    }

    fn patterns(&self) -> impl Iterator<Item = &String> {
        [&self.property, &self.pset, &self.attribute]
            .into_iter()
            .flatten()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InformationItem {
    pub id: String,
    /// Name shown in results, in the language of the standard.
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The item is present when any of these has a value.
    pub any: Vec<Source>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingPart {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The part applies to elements any of these selectors match.
    pub applies: Vec<Selector>,
    /// Items required per level, by item id. A level that is not listed
    /// requires nothing for this part.
    pub levels: BTreeMap<String, Vec<String>>,
    /// Where items are found for this part, by item id. Replaces the item's
    /// own sources, e.g. a width that cable carriers keep in another property.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sources: BTreeMap<String, Vec<Source>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InformationStandard {
    pub standard: StandardInfo,
    #[serde(rename = "item", default)]
    pub items: Vec<InformationItem>,
    #[serde(rename = "part", default)]
    pub parts: Vec<BuildingPart>,
}

impl InformationStandard {
    pub fn from_toml(text: &str) -> Result<Self, RulesetError> {
        let standard: InformationStandard = toml::from_str(text)?;
        standard.validate()?;
        Ok(standard)
    }

    pub fn item(&self, id: &str) -> Option<&InformationItem> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Where an item is found for a part: the part's own sources for it, or
    /// the item's.
    pub fn sources_for<'a>(
        &'a self,
        part: &'a BuildingPart,
        item: &'a InformationItem,
    ) -> &'a [Source] {
        part.sources.get(&item.id).unwrap_or(&item.any)
    }

    /// The part an element belongs to: the first that applies.
    pub fn part_of(&self, model: &IfcModel, element: &Element) -> Option<&BuildingPart> {
        self.parts
            .iter()
            .find(|p| p.applies.iter().any(|s| s.matches(model, element)))
    }

    fn validate(&self) -> Result<(), RulesetError> {
        let invalid = |m: String| Err(RulesetError::Invalid(m));
        let info = &self.standard;
        if info.id.trim().is_empty() || info.levels.is_empty() {
            return invalid("the standard needs an id and at least one level".into());
        }
        if !info.levels.contains(&info.default_level) {
            return invalid(format!(
                "default_level '{}' is not one of the levels",
                info.default_level
            ));
        }
        let mut item_ids = FxHashSet::default();
        for item in &self.items {
            if !item_ids.insert(item.id.as_str()) {
                return invalid(format!("item id '{}' is used twice", item.id));
            }
            validate_sources(&format!("item '{}'", item.id), &item.any)?;
        }
        let mut part_ids = FxHashSet::default();
        for part in &self.parts {
            if !part_ids.insert(part.id.as_str()) {
                return invalid(format!("part id '{}' is used twice", part.id));
            }
            if part.applies.is_empty() {
                return invalid(format!("part '{}' applies to nothing", part.id));
            }
            for (item, sources) in &part.sources {
                if !item_ids.contains(item.as_str()) {
                    return invalid(format!(
                        "part '{}': sources for unknown item '{item}'",
                        part.id
                    ));
                }
                validate_sources(&format!("part '{}', item '{item}'", part.id), sources)?;
            }
            for (level, items) in &part.levels {
                if !info.levels.contains(level) {
                    return invalid(format!("part '{}': unknown level '{level}'", part.id));
                }
                if let Some(missing) = items.iter().find(|i| !item_ids.contains(i.as_str())) {
                    return invalid(format!("part '{}': unknown item '{missing}'", part.id));
                }
            }
        }
        Ok(())
    }
}

/// Checks a list of sources; `context` names it in errors.
pub fn validate_sources(context: &str, sources: &[Source]) -> Result<(), RulesetError> {
    let invalid = |m: String| Err(RulesetError::Invalid(m));
    if sources.is_empty() {
        return invalid(format!("{context} has no sources"));
    }
    for source in sources {
        if source.kinds() != 1 {
            return invalid(format!(
                "{context}: each source sets exactly one of property, attribute, \
                 type_name, classification, material, system, storey or geometry"
            ));
        }
        if source.pset.is_some() && source.property.is_none() {
            return invalid(format!("{context}: pset needs property"));
        }
        for pattern in source.patterns() {
            if let Err(e) = Regex::new(pattern) {
                return invalid(format!("{context}: bad pattern {pattern:?}: {e}"));
            }
        }
    }
    Ok(())
}

/// Information rules check elements on their own: parts of assemblies follow
/// their assembly, and elements without geometry or parts are left out.
pub fn is_checked(element: &Element) -> bool {
    element.parent.is_none() && (element.has_geometry() || !element.parts.is_empty())
}

/// A standard that ships with OpenCheck.
pub struct BuiltinStandard {
    pub id: &'static str,
    pub toml: &'static str,
}

pub const BUILTIN_STANDARDS: &[BuiltinStandard] = &[BuiltinStandard {
    id: "opencheck.information.lod-dk",
    toml: include_str!("../../standards/information-lod-dk.toml"),
}];

/// A built-in standard by id, parsed once.
pub fn builtin_standard(id: &str) -> Option<&'static InformationStandard> {
    static PARSED: LazyLock<Vec<InformationStandard>> = LazyLock::new(|| {
        BUILTIN_STANDARDS
            .iter()
            .map(|b| InformationStandard::from_toml(b.toml).expect("built-in standards parse"))
            .collect()
    });
    PARSED.iter().find(|s| s.standard.id == id)
}

/// A compiled, case-insensitive pattern. Patterns are few and reused for
/// every element, so they are compiled once.
pub(crate) fn pattern(text: &str) -> Option<Regex> {
    static CACHE: LazyLock<Mutex<FxHashMap<String, Option<Regex>>>> =
        LazyLock::new(Default::default);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(text.to_owned())
        .or_insert_with(|| Regex::new(&format!("(?i){text}")).ok())
        .clone()
}

pub(crate) fn matches_pattern(pattern_text: &str, value: &str) -> bool {
    pattern(pattern_text).is_some_and(|re| re.is_match(value))
}

/// Matches a property or attribute name as written and with spaces and
/// underscores removed, so `^InsulationThickness$` also finds Revit's
/// "Insulation Thickness".
fn matches_name(pattern_text: &str, name: &str) -> bool {
    if matches_pattern(pattern_text, name) {
        return true;
    }
    let compact: String = name
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect();
    compact.len() != name.len() && matches_pattern(pattern_text, &compact)
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

/// What a model knows that the sources need, computed once per model.
pub struct ModelFacts {
    storeys: FxHashSet<u32>,
}

impl ModelFacts {
    pub fn new(model: &IfcModel) -> Self {
        Self {
            storeys: model.storeys().iter().map(|s| s.express_id).collect(),
        }
    }
}

impl Source {
    /// Where the element has this information, e.g. `Pset_WallCommon.Width`
    /// or `Classification CCS`; None when it does not have it.
    pub(crate) fn found(
        &self,
        model: &IfcModel,
        facts: &ModelFacts,
        element: &Element,
    ) -> Option<String> {
        if let Some(name) = &self.property {
            return model.property_sets_of(element).find_map(|pset| {
                if !self
                    .pset
                    .as_ref()
                    .is_none_or(|p| matches_pattern(p, &pset.name))
                {
                    return None;
                }
                pset.properties
                    .iter()
                    .find(|p| matches_name(name, &p.name) && has_value(&p.value))
                    .map(|p| format!("{}.{}", pset.name, p.name))
            });
        }
        if let Some(name) = &self.attribute {
            return element
                .attributes
                .iter()
                .find(|(n, v)| matches_name(name, n) && has_value(v))
                .map(|(n, _)| format!("Attribute {n}"));
        }
        if self.type_name {
            return element
                .type_object
                .as_ref()
                .filter(|(_, name)| !name.trim().is_empty())
                .map(|_| "Type name".to_owned());
        }
        if self.classification {
            return element
                .classifications
                .iter()
                .find(|c| !c.code.is_empty() || !c.name.is_empty())
                .map(|c| {
                    if c.system.is_empty() {
                        "Classification".to_owned()
                    } else {
                        format!("Classification {}", c.system)
                    }
                });
        }
        if self.material {
            return (!element.materials.is_empty()).then(|| "Material".to_owned());
        }
        if self.system {
            return (!element.systems.is_empty()).then(|| "System".to_owned());
        }
        if self.storey {
            return element
                .container
                .is_some_and(|c| facts.storeys.contains(&c))
                .then(|| "Storey".to_owned());
        }
        if self.geometry {
            return element.has_geometry().then(|| "Geometry".to_owned());
        }
        None
    }

    /// The source in words, e.g. `Property ^Width$ in ^Pset_` or `Classification`.
    pub fn label(&self) -> String {
        if let Some(name) = &self.property {
            return match &self.pset {
                Some(pset) => format!(
                    "Property {} in {}",
                    display_pattern(name),
                    display_pattern(pset)
                ),
                None => format!("Property {}", display_pattern(name)),
            };
        }
        if let Some(name) = &self.attribute {
            return format!("Attribute {}", display_pattern(name));
        }
        let label = if self.type_name {
            "Type name"
        } else if self.classification {
            "Classification"
        } else if self.material {
            "Material"
        } else if self.system {
            "System"
        } else if self.storey {
            "Storey"
        } else if self.geometry {
            "Geometry"
        } else {
            "Nothing"
        };
        label.to_owned()
    }
}

/// A pattern for people: an exact name (`^Width$`) as the name itself.
pub fn display_pattern(pattern: &str) -> String {
    match exact_name(pattern) {
        Some(name) => name,
        None => format!("/{pattern}/"),
    }
}

/// The name an anchored pattern without special characters stands for.
pub fn exact_name(pattern: &str) -> Option<String> {
    let inner = pattern.strip_prefix('^')?.strip_suffix('$')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?),
            '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' => {
                return None;
            }
            c => out.push(c),
        }
    }
    Some(out)
}

/// The pattern that matches exactly this name.
pub fn exact_pattern(name: &str) -> String {
    format!("^{}$", regex::escape(name.trim()))
}

impl InformationStandard {
    /// The part an element is checked as and where it has an item, e.g.
    /// `Some((wall, Some("Pset_WallCommon.LoadBearing")))`; the inner None
    /// means the item is missing. None when the element is not checked or
    /// no part applies.
    pub fn item_on<'a>(
        &'a self,
        model: &IfcModel,
        facts: &ModelFacts,
        element: &Element,
        item: &str,
    ) -> Option<(&'a BuildingPart, Option<String>)> {
        if !is_checked(element) {
            return None;
        }
        let part = self.part_of(model, element)?;
        let item = self.item(item)?;
        Some((part, self.found(part, item, model, facts, element)))
    }

    /// Where an element has an item for a part: the first source that has
    /// it, described. None when the item is missing.
    pub(crate) fn found(
        &self,
        part: &BuildingPart,
        item: &InformationItem,
        model: &IfcModel,
        facts: &ModelFacts,
        element: &Element,
    ) -> Option<String> {
        self.sources_for(part, item)
            .iter()
            .find_map(|s| s.found(model, facts, element))
    }
}

/// How well one item is filled in across the elements of a part.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemCoverage {
    pub item: String,
    pub name: String,
    /// Elements that needed the item.
    pub required: usize,
    /// Of those, elements that have it.
    pub present: usize,
    /// Where it was found and on how many elements, most common first,
    /// e.g. `("Pset_WallCommon.Width", 120)`.
    pub found_in: Vec<(String, usize)>,
}

/// Results for one kind of building part.
#[derive(Debug, Clone, PartialEq)]
pub struct PartCoverage {
    pub part: String,
    pub name: String,
    /// Elements that belong to the part.
    pub elements: usize,
    /// Elements that have every required item.
    pub complete: usize,
    pub items: Vec<ItemCoverage>,
}

/// Summary of an information rule: the levels used and the fill rate per part.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InformationSummary {
    pub standard: String,
    pub parts: Vec<PartCoverage>,
    /// Problems that concern no single element, e.g. a missing IDS file or
    /// an IDS specification that requires elements the models lack.
    pub notes: Vec<String>,
}

/// A project's changes to a built-in standard: where items are found, for
/// all parts or for one part. Stored in the project, applied on top of the
/// built-in standard so its own updates still arrive.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardOverrides {
    /// Id of the standard the changes apply to.
    pub base: String,
    /// Sources per item id, replacing the item's own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub items: BTreeMap<String, Vec<Source>>,
    /// Sources per part id and item id, replacing those of the item for that part.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parts: BTreeMap<String, BTreeMap<String, Vec<Source>>>,
}

impl StandardOverrides {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.to_owned(),
            ..Self::default()
        }
    }

    pub fn from_toml(text: &str) -> Result<Self, RulesetError> {
        let overrides: StandardOverrides = toml::from_str(text)?;
        for (item, sources) in &overrides.items {
            validate_sources(&format!("item '{item}'"), sources)?;
        }
        for (part, items) in &overrides.parts {
            for (item, sources) in items {
                validate_sources(&format!("part '{part}', item '{item}'"), sources)?;
            }
        }
        Ok(overrides)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("overrides always serialise")
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.parts.values().all(BTreeMap::is_empty)
    }

    /// The standard with these changes. Changes to items or parts the
    /// standard no longer has are skipped and returned as messages.
    pub fn apply(&self, base: &InformationStandard) -> (InformationStandard, Vec<String>) {
        let mut standard = base.clone();
        let mut skipped = Vec::new();
        for (id, sources) in &self.items {
            match standard.items.iter_mut().find(|i| &i.id == id) {
                Some(item) => item.any = sources.clone(),
                None => skipped.push(format!("The standard has no item '{id}' any more.")),
            }
        }
        for (part_id, items) in &self.parts {
            let Some(part) = standard.parts.iter_mut().find(|p| &p.id == part_id) else {
                skipped.push(format!("The standard has no part '{part_id}' any more."));
                continue;
            };
            for (item, sources) in items {
                if base.item(item).is_none() {
                    skipped.push(format!("The standard has no item '{item}' any more."));
                    continue;
                }
                part.sources.insert(item.clone(), sources.clone());
            }
        }
        (standard, skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_standards_parse() {
        for b in BUILTIN_STANDARDS {
            let standard = builtin_standard(b.id).expect("listed by id");
            assert!(!standard.parts.is_empty());
        }
    }

    #[test]
    fn rejects_unknown_items_levels_and_ambiguous_sources() {
        let base = r#"
            [standard]
            id = "t"
            name = "T"
            levels = ["200", "300"]
            default_level = "300"

            [[item]]
            id = "code"
            name = "Code"
            any = [{ property = "^Code$" }]
        "#;
        let part = |levels: &str| {
            format!(
                "{base}\n[[part]]\nid = \"wall\"\nname = \"Wall\"\napplies = [{{ classes = [\"IfcWall\"] }}]\n[part.levels]\n{levels}\n"
            )
        };
        assert!(InformationStandard::from_toml(&part("\"300\" = [\"code\"]")).is_ok());
        assert!(InformationStandard::from_toml(&part("\"300\" = [\"nope\"]")).is_err());
        assert!(InformationStandard::from_toml(&part("\"500\" = [\"code\"]")).is_err());
        let both = base.replace(
            "{ property = \"^Code$\" }",
            "{ property = \"^Code$\", classification = true }",
        );
        assert!(InformationStandard::from_toml(&both).is_err());
        let bad_regex = base.replace("^Code$", "(");
        assert!(InformationStandard::from_toml(&bad_regex).is_err());
    }

    #[test]
    fn patterns_ignore_case_and_cover_danish_letters() {
        assert!(matches_pattern("^bredde$", "Bredde"));
        assert!(matches_pattern("^højde$", "HØJDE"));
        assert!(!matches_pattern("^CCSClassCode$", "CCSClassCodeVersion"));
        assert!(matches_name(
            "^InsulationThickness$",
            "Insulation Thickness"
        ));
        assert!(matches_name("^NominalWidth$", "Nominal_Width"));
        assert!(!has_value(&PropertyValue::Text("  ".into())));
        assert!(has_value(&PropertyValue::Boolean(false)));
    }

    #[test]
    fn exact_names_and_overrides() {
        assert_eq!(exact_name("^Width$").as_deref(), Some("Width"));
        assert_eq!(
            exact_name(&exact_pattern("Bredde (mm)")).as_deref(),
            Some("Bredde (mm)")
        );
        assert_eq!(exact_name("^(Width|Height)$"), None);
        assert_eq!(display_pattern("Width"), "/Width/");
        assert_eq!(
            Source {
                property: Some("^Width$".into()),
                pset: Some("^Pset_WallCommon$".into()),
                ..Source::default()
            }
            .label(),
            "Property Width in Pset_WallCommon"
        );

        let base = builtin_standard("opencheck.information.lod-dk").unwrap();
        let mut overrides = StandardOverrides::new(&base.standard.id);
        assert!(overrides.is_empty());
        overrides.parts.entry("nope".into()).or_default();
        overrides.items.insert(
            "gone".into(),
            vec![Source {
                material: true,
                ..Source::default()
            }],
        );
        let (adapted, skipped) = overrides.apply(base);
        assert_eq!(skipped.len(), 2);
        assert_eq!(adapted.items, base.items);
        // Bad sources are refused.
        let bad = "base = \"x\"
[items]
width = [{ material = true, system = true }]
";
        assert!(StandardOverrides::from_toml(bad).is_err());
    }

    /// The overrides example in the documentation applies cleanly.
    #[test]
    fn documented_overrides_example_applies() {
        let doc = include_str!("../../../../docs/information-requirements.md");
        let example = doc
            .split("```toml")
            .filter_map(|rest| rest.split("```").next())
            .find(|block| block.contains("base = "))
            .expect("example in the docs");
        let overrides = StandardOverrides::from_toml(example).unwrap();
        let base = builtin_standard(&overrides.base).unwrap();
        let (_, skipped) = overrides.apply(base);
        assert!(skipped.is_empty(), "{skipped:?}");
    }
}
