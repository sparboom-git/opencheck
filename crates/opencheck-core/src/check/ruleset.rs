//! Rulesets: what to check, written as TOML.
//!
//! The format is documented for users (and for language models writing
//! custom rulesets) in `docs/ruleset-authoring.md`. Keep the two in step.

use serde::{Deserialize, Serialize};

use super::ids::ids_path;
use super::information::{builtin_standard, matches_pattern};
use crate::{Discipline, Element, IfcModel, PropertyValue};

/// How serious a finding is. Matches the three levels used by Solibri and BCF
/// priority conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    Moderate,
    Low,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Critical => "Critical",
            Severity::Moderate => "Moderate",
            Severity::Low => "Low",
        }
    }
}

/// Which pairs of elements a rule may compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Between {
    /// Any two elements, also from the same model.
    #[default]
    Any,
    /// Only elements from different models.
    DifferentModels,
    /// Only elements from the same model.
    SameModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    /// Solid elements must not intersect.
    Clash,
    /// Elements must keep a minimum distance.
    Clearance,
    /// The same element must not be modelled twice.
    Duplicates,
    /// Building parts must carry the information an information standard
    /// requires at the chosen level (LOD).
    Information,
    /// A zone beside, above or below each element of group `a` must be free
    /// of group `b`, e.g. the service area along a cable tray.
    FreeZone,
    /// A rule written as a script (Rhai) with its own parameters.
    Script,
}

/// Where a free zone lies, seen along the element's run (its longest axis).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneSide {
    /// Over the element.
    #[default]
    Above,
    /// Under the element.
    Below,
    /// On both sides; each side must be free.
    BothSides,
    /// On the sides; at least one of them must be free.
    OneSide,
    /// In front of the element's broad face, on either face: for vertical
    /// runs, e.g. the service area in front of a vertical cable ladder.
    OneFace,
}

impl ZoneSide {
    pub const ALL: [ZoneSide; 5] = [
        ZoneSide::Above,
        ZoneSide::Below,
        ZoneSide::BothSides,
        ZoneSide::OneSide,
        ZoneSide::OneFace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ZoneSide::Above => "Above",
            ZoneSide::Below => "Below",
            ZoneSide::BothSides => "Both sides",
            ZoneSide::OneSide => "At least one side",
            ZoneSide::OneFace => "In front (one face)",
        }
    }
}

/// Which elements a free zone rule applies to, by the direction of their run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunDirection {
    #[default]
    Any,
    /// Runs within 30 degrees of horizontal.
    Horizontal,
    /// Runs within 30 degrees of vertical.
    Vertical,
}

impl RunDirection {
    pub fn label(self) -> &'static str {
        match self {
            RunDirection::Any => "Any",
            RunDirection::Horizontal => "Horizontal",
            RunDirection::Vertical => "Vertical",
        }
    }
}

/// The zone a free zone rule keeps clear.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Zone {
    #[serde(default)]
    pub side: ZoneSide,
    /// How far the zone reaches out from the element.
    pub depth_mm: f32,
    /// The zone may be blocked over at most this length along the run.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_blocked_mm: f32,
    /// Even where the zone is blocked, at least this much must stay free.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min_free_mm: f32,
    #[serde(default)]
    pub direction: RunDirection,
}

impl Default for Zone {
    fn default() -> Self {
        Self {
            side: ZoneSide::Above,
            depth_mm: 200.0,
            max_blocked_mm: 0.0,
            min_free_mm: 0.0,
            direction: RunDirection::Any,
        }
    }
}

/// A property condition, e.g. `Pset_WallCommon.LoadBearing = true`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyFilter {
    /// Property set name; empty matches any set.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pset: String,
    pub name: String,
    /// Value to compare with, case-insensitive. Omitted: the property must exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<String>,
}

/// Selects the elements on one side of a rule.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Selector {
    /// Model disciplines; empty means any discipline.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disciplines: Vec<Discipline>,
    /// IFC classes; subtypes match too (`IfcWall` matches `IfcWallStandardCase`).
    /// Empty means any class.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
    /// IFC classes to leave out, subtypes included.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_classes: Vec<String>,
    /// All filters must match.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<PropertyFilter>,
    /// Patterns (regular expressions, ignoring case) for the element's name,
    /// object type or type name; one must match. Empty means any name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// Patterns for material names; one must match a material of the element.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<String>,
}

impl Selector {
    pub fn matches(&self, model: &IfcModel, element: &Element) -> bool {
        if !self.disciplines.is_empty() && !self.disciplines.contains(&model.discipline) {
            return false;
        }
        // IFC2x3 elements also match under the class their type gives them,
        // so `IfcCableCarrierSegment` finds Revit's typed `IfcFlowSegment`s.
        let occurrence = element.occurrence_class();
        let is_a = |class: &String| {
            is_subtype(&model.schema, &element.ifc_class, class)
                || (occurrence != element.ifc_class && is_subtype(&model.schema, occurrence, class))
        };
        if !self.classes.is_empty() && !self.classes.iter().any(is_a) {
            return false;
        }
        if self.exclude_classes.iter().any(is_a) {
            return false;
        }
        if !self.names.is_empty() {
            let type_name = element.type_object.as_ref().map(|(_, n)| n.as_str());
            let candidates = [
                Some(element.name.as_str()),
                element.object_type.as_deref(),
                type_name,
            ];
            let named = self.names.iter().any(|p| {
                candidates
                    .iter()
                    .flatten()
                    .any(|name| matches_pattern(p, name))
            });
            if !named {
                return false;
            }
        }
        if !self.materials.is_empty()
            && !self
                .materials
                .iter()
                .any(|p| element.materials.iter().any(|m| matches_pattern(p, m)))
        {
            return false;
        }
        self.properties
            .iter()
            .all(|filter| property_matches(model, element, filter))
    }
}

pub(super) fn is_subtype(schema: &str, class: &str, parent: &str) -> bool {
    class.eq_ignore_ascii_case(parent)
        || ifc_lite_core::is_subtype_of_for_schema(schema, class, parent)
        // IFC4X3 renamed IfcBuildingElement; accept either name everywhere.
        || (parent.eq_ignore_ascii_case("IfcBuildingElement")
            && ifc_lite_core::is_subtype_of_for_schema(schema, class, "IfcBuiltElement"))
        || (parent.eq_ignore_ascii_case("IfcBuiltElement")
            && ifc_lite_core::is_subtype_of_for_schema(schema, class, "IfcBuildingElement"))
}

fn property_matches(model: &IfcModel, element: &Element, filter: &PropertyFilter) -> bool {
    model.property_sets_of(element).any(|pset| {
        (filter.pset.is_empty() || pset.name.eq_ignore_ascii_case(&filter.pset))
            && pset
                .get(&filter.name)
                .is_some_and(|p| match &filter.equals {
                    None => true,
                    Some(expected) => value_equals(&p.value, expected),
                })
    })
}

fn value_equals(value: &PropertyValue, expected: &str) -> bool {
    let expected = expected.trim();
    match value {
        PropertyValue::Boolean(b) | PropertyValue::Logical(Some(b)) => {
            let truthy = matches!(expected.to_ascii_lowercase().as_str(), "true" | "yes" | "1");
            *b == truthy
        }
        PropertyValue::Integer(i) => expected.parse::<i64>().is_ok_and(|e| e == *i),
        PropertyValue::Real(r) => expected
            .parse::<f64>()
            .is_ok_and(|e| (e - r).abs() <= 1e-6 * e.abs().max(1.0)),
        other => other.to_string().eq_ignore_ascii_case(expected),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Unique within the ruleset; used to keep decisions between runs.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(rename = "type")]
    pub kind: RuleKind,
    pub severity: Severity,
    /// Clash: intersections shallower than this are ignored.
    /// Duplicates: how far apart two copies may be.
    #[serde(default = "default_tolerance")]
    pub tolerance_mm: f32,
    /// Clearance: the required free distance.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clearance_mm: f32,
    #[serde(default)]
    pub between: Between,
    /// First group of elements.
    #[serde(default)]
    pub a: Selector,
    /// Second group. Omitted: elements of group `a` are checked against each other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Selector>,
    /// Information: id of a built-in information standard, or `ids:<path>`
    /// for an IDS file stored in the project, e.g. `ids:requirements/x.ids`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub standard: String,
    /// Free zone: the zone to keep clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<Zone>,
    /// Script: the rule's code (Rhai).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub script: String,
    /// Script: values the script reads with `param("name")`, shown as input
    /// fields in the app. Lengths are in millimetres by convention.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: std::collections::BTreeMap<String, f64>,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn default_tolerance() -> f32 {
    10.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesetInfo {
    /// Stable identifier, e.g. `opencheck.clash.mep-vs-structure`.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
}

fn default_version() -> String {
    "1".to_owned()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ruleset {
    pub ruleset: RulesetInfo,
    #[serde(rename = "rule", default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, thiserror::Error)]
pub enum RulesetError {
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    #[error("{0}")]
    Invalid(String),
}

impl Ruleset {
    /// The ruleset as TOML, in the same layout people write by hand.
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("rulesets always serialise")
    }

    /// Checks the ruleset without parsing, for editors that build one in memory.
    pub fn check(&self) -> Result<(), RulesetError> {
        self.validate()
    }

    pub fn from_toml(text: &str) -> Result<Self, RulesetError> {
        let ruleset: Ruleset = toml::from_str(text)?;
        ruleset.validate()?;
        Ok(ruleset)
    }

    fn validate(&self) -> Result<(), RulesetError> {
        let invalid = |m: String| Err(RulesetError::Invalid(m));
        if self.ruleset.id.trim().is_empty() {
            return invalid("the ruleset needs an id".into());
        }
        if self.rules.is_empty() {
            return invalid("the ruleset has no [[rule]] entries".into());
        }
        let mut ids = std::collections::HashSet::new();
        for rule in &self.rules {
            if !ids.insert(rule.id.as_str()) {
                return invalid(format!("rule id '{}' is used twice", rule.id));
            }
            if rule.kind == RuleKind::Clearance && rule.clearance_mm <= 0.0 {
                return invalid(format!("rule '{}': clearance_mm must be above 0", rule.id));
            }
            if rule.kind == RuleKind::Information
                && builtin_standard(&rule.standard).is_none()
                && ids_path(&rule.standard).is_none()
            {
                return invalid(format!(
                    "rule '{}': unknown information standard '{}'",
                    rule.id, rule.standard
                ));
            }
            if rule.kind == RuleKind::FreeZone {
                match &rule.zone {
                    None => {
                        return invalid(format!(
                            "rule '{}': a free_zone rule needs a [rule.zone] table",
                            rule.id
                        ));
                    }
                    Some(zone) if zone.depth_mm <= 0.0 => {
                        return invalid(format!(
                            "rule '{}': zone.depth_mm must be above 0",
                            rule.id
                        ));
                    }
                    Some(zone) if zone.min_free_mm >= zone.depth_mm => {
                        return invalid(format!(
                            "rule '{}': zone.min_free_mm must be less than zone.depth_mm",
                            rule.id
                        ));
                    }
                    _ => {}
                }
            }
            if rule.kind == RuleKind::Script {
                if rule.script.trim().is_empty() {
                    return invalid(format!("rule '{}': a script rule needs a script", rule.id));
                }
                if let Err(e) = super::script::compile(&rule.script) {
                    return invalid(format!("rule '{}': {e}", rule.id));
                }
            }
            if rule.tolerance_mm < 0.0 {
                return invalid(format!(
                    "rule '{}': tolerance_mm cannot be negative",
                    rule.id
                ));
            }
        }
        Ok(())
    }
}

/// A ruleset that ships with OpenCheck.
pub struct BuiltinRuleset {
    pub file_name: &'static str,
    pub toml: &'static str,
}

/// The default rulesets, in the order they are offered.
pub const BUILTIN_RULESETS: &[BuiltinRuleset] = &[
    BuiltinRuleset {
        file_name: "clash-interdisciplinary.toml",
        toml: include_str!("../../rulesets/clash-interdisciplinary.toml"),
    },
    BuiltinRuleset {
        file_name: "clash-internal.toml",
        toml: include_str!("../../rulesets/clash-internal.toml"),
    },
    BuiltinRuleset {
        file_name: "clearance-mep-structure.toml",
        toml: include_str!("../../rulesets/clearance-mep-structure.toml"),
    },
    BuiltinRuleset {
        file_name: "electrical-cable-trays.toml",
        toml: include_str!("../../rulesets/electrical-cable-trays.toml"),
    },
    BuiltinRuleset {
        file_name: "ventilation-clearances.toml",
        toml: include_str!("../../rulesets/ventilation-clearances.toml"),
    },
    BuiltinRuleset {
        file_name: "information-lod-dk.toml",
        toml: include_str!("../../rulesets/information-lod-dk.toml"),
    },
    BuiltinRuleset {
        file_name: "model-quality-duplicates.toml",
        toml: include_str!("../../rulesets/model-quality-duplicates.toml"),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_rulesets_parse() {
        for builtin in BUILTIN_RULESETS {
            let ruleset = Ruleset::from_toml(builtin.toml)
                .unwrap_or_else(|e| panic!("{}: {e}", builtin.file_name));
            assert!(!ruleset.rules.is_empty());
        }
    }

    #[test]
    fn toml_round_trip() {
        for builtin in BUILTIN_RULESETS {
            let ruleset = Ruleset::from_toml(builtin.toml).unwrap();
            let again = Ruleset::from_toml(&ruleset.to_toml()).unwrap_or_else(|e| {
                panic!(
                    "{}: {e}
{}",
                    builtin.file_name,
                    ruleset.to_toml()
                )
            });
            assert_eq!(ruleset, again);
        }
    }

    /// Every complete example in the authoring guide must be a valid ruleset.
    #[test]
    fn authoring_guide_examples_parse() {
        let guide = include_str!("../../../../docs/ruleset-authoring.md");
        let blocks: Vec<&str> = guide
            .split("```toml")
            .skip(1)
            .filter_map(|rest| rest.split("```").next())
            .filter(|block| block.contains("[ruleset]"))
            .collect();
        assert!(blocks.len() >= 4, "found {} examples", blocks.len());
        for block in blocks {
            Ruleset::from_toml(block).unwrap_or_else(|e| panic!("{e}\n{block}"));
        }
    }

    #[test]
    fn rejects_unknown_fields_and_bad_values() {
        let text = r#"
            [ruleset]
            id = "x"
            name = "X"
            [[rule]]
            id = "r"
            name = "R"
            type = "clearance"
            severity = "low"
        "#;
        assert!(matches!(
            Ruleset::from_toml(text),
            Err(RulesetError::Invalid(_))
        ));
        let typo = text.replace("severity", "severty");
        assert!(matches!(
            Ruleset::from_toml(&typo),
            Err(RulesetError::Toml(_))
        ));
    }
}
