//! Rulesets: what to check, written as TOML.
//!
//! The format is documented for users (and for language models writing
//! custom rulesets) in `docs/ruleset-authoring.md`. Keep the two in step.

use serde::{Deserialize, Serialize};

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
}

impl Selector {
    pub fn matches(&self, model: &IfcModel, element: &Element) -> bool {
        if !self.disciplines.is_empty() && !self.disciplines.contains(&model.discipline) {
            return false;
        }
        let is_a = |class: &String| is_subtype(&model.schema, &element.ifc_class, class);
        if !self.classes.is_empty() && !self.classes.iter().any(is_a) {
            return false;
        }
        if self.exclude_classes.iter().any(is_a) {
            return false;
        }
        self.properties
            .iter()
            .all(|filter| property_matches(model, element, filter))
    }
}

fn is_subtype(schema: &str, class: &str, parent: &str) -> bool {
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
        file_name: "clash-mep-vs-structure.toml",
        toml: include_str!("../../rulesets/clash-mep-vs-structure.toml"),
    },
    BuiltinRuleset {
        file_name: "clash-mep-vs-architecture.toml",
        toml: include_str!("../../rulesets/clash-mep-vs-architecture.toml"),
    },
    BuiltinRuleset {
        file_name: "clash-structure-vs-architecture.toml",
        toml: include_str!("../../rulesets/clash-structure-vs-architecture.toml"),
    },
    BuiltinRuleset {
        file_name: "clash-mep-coordination.toml",
        toml: include_str!("../../rulesets/clash-mep-coordination.toml"),
    },
    BuiltinRuleset {
        file_name: "clearance-mep-structure.toml",
        toml: include_str!("../../rulesets/clearance-mep-structure.toml"),
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
