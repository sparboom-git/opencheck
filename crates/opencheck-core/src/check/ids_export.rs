//! Writes an information standard as IDS 1.0.
//!
//! The TOML format says more than IDS can, so the export loses some of it.
//! Each loss is reported once as a warning; the full list is in
//! `docs/information-requirements.md`.

use super::ids::{
    Cardinality, EntityFacet, Facet, FacetKind, Ids, IdsInfo, IdsValue, Restriction, Specification,
    compile_pattern,
};
use super::information::{InformationItem, InformationStandard, Source};
use super::ruleset::Selector;

/// The IDS document and what it could not express.
pub struct IdsExport {
    pub ids: Ids,
    pub warnings: Vec<String>,
}

/// IFC versions the exported specifications are written for.
const VERSIONS: &[&str] = &["IFC2X3", "IFC4", "IFC4X3_ADD2"];

/// Exports the given levels of a standard (all levels when empty). Each part
/// and level becomes a specification; a part with several selectors becomes
/// one specification per selector.
pub fn standard_to_ids(standard: &InformationStandard, levels: &[String]) -> IdsExport {
    let mut warnings = Warnings::default();
    let levels: Vec<&String> = standard
        .standard
        .levels
        .iter()
        .filter(|l| levels.is_empty() || levels.contains(l))
        .collect();
    let mut specifications = Vec::new();
    warnings.add(
        "Parts are checked independently in IDS: an element that more than one part applies to \
         is checked against each of them, not only the first.",
    );

    for level in &levels {
        for part in &standard.parts {
            let Some(item_ids) = part.levels.get(*level) else {
                continue;
            };
            let requirements: Vec<Facet> = item_ids
                .iter()
                .filter_map(|id| standard.item(id))
                .filter_map(|item| {
                    item_facet(item, standard.sources_for(part, item), &mut warnings)
                })
                .collect();
            if requirements.is_empty() {
                continue;
            }
            for (n, selector) in part.applies.iter().enumerate() {
                let applicability = selector_facets(selector, &part.name, &mut warnings);
                if applicability.is_empty() {
                    warnings.add(format!(
                        "Part '{}': a selector without classes, names, materials or properties \
                         was left out.",
                        part.name
                    ));
                    continue;
                }
                let suffix = if part.applies.len() > 1 {
                    format!(" ({})", n + 1)
                } else {
                    String::new()
                };
                let mut description = part.description.clone();
                if !selector.disciplines.is_empty() {
                    let list: Vec<&str> = selector.disciplines.iter().map(|d| d.code()).collect();
                    let note = format!("Applies to {} models.", list.join(", "));
                    description = if description.is_empty() {
                        note
                    } else {
                        format!("{description} {note}")
                    };
                }
                specifications.push(Specification {
                    name: format!("{} – LOD {level}{suffix}", part.name),
                    identifier: format!(
                        "{}@{level}{}",
                        part.id,
                        suffix.replace([' ', '(', ')'], "")
                    ),
                    description,
                    ifc_versions: VERSIONS.iter().map(|v| (*v).to_owned()).collect(),
                    min_occurs: 0,
                    max_occurs: None,
                    applicability,
                    requirements: requirements.clone(),
                    ..Specification::default()
                });
            }
        }
    }

    let milestone = match levels.as_slice() {
        [one] => format!("LOD {one}"),
        _ => String::new(),
    };
    IdsExport {
        ids: Ids {
            info: IdsInfo {
                title: standard.standard.name.clone(),
                description: standard.standard.description.clone(),
                version: standard.standard.version.clone(),
                author: String::new(),
                milestone,
                purpose: "Information requirements".to_owned(),
                ..IdsInfo::default()
            },
            specifications,
        },
        warnings: warnings.0,
    }
}

#[derive(Default)]
struct Warnings(Vec<String>);

impl Warnings {
    fn add(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.0.contains(&message) {
            self.0.push(message);
        }
    }
}

fn required(kind: FacetKind) -> Facet {
    Facet {
        kind,
        cardinality: Cardinality::Required,
        instructions: String::new(),
        uri: String::new(),
    }
}

/// IDS entity names of a class and all its subtypes in any supported IFC
/// version, upper case and sorted.
fn class_names(class: &str) -> Vec<String> {
    let mut names: Vec<String> = ifc_lite_core::IFC_TYPES
        .iter()
        .map(|t| t.name())
        .filter(|name| {
            ["IFC2X3", "IFC4", "IFC4X3_ADD2"]
                .iter()
                .any(|schema| ifc_lite_core::is_subtype_of_for_schema(schema, name, class))
                // IFC4X3 renamed IfcBuildingElement to IfcBuiltElement.
                || (class.eq_ignore_ascii_case("IfcBuildingElement")
                    && ifc_lite_core::is_subtype_of_for_schema("IFC4X3_ADD2", name, "IfcBuiltElement"))
        })
        .map(str::to_ascii_uppercase)
        .collect();
    if names.is_empty() {
        names.push(class.to_ascii_uppercase());
    }
    names.sort();
    names.dedup();
    names
}

fn selector_facets(selector: &Selector, part: &str, warnings: &mut Warnings) -> Vec<Facet> {
    let mut facets = Vec::new();
    if !selector.classes.is_empty() {
        let excluded: Vec<String> = selector
            .exclude_classes
            .iter()
            .flat_map(|c| class_names(c))
            .collect();
        let mut names: Vec<String> = selector
            .classes
            .iter()
            .flat_map(|c| class_names(c))
            .filter(|n| !excluded.contains(n))
            .collect();
        names.sort();
        names.dedup();
        let name = match names.as_slice() {
            [one] => IdsValue::Simple(one.clone()),
            _ => IdsValue::Restriction(Restriction::enumeration(names)),
        };
        facets.push(required(FacetKind::Entity(EntityFacet {
            name,
            predefined_type: None,
        })));
    } else if !selector.exclude_classes.is_empty() {
        warnings.add(format!(
            "Part '{part}': excluded classes without included classes cannot be written in IDS."
        ));
    }
    if !selector.names.is_empty() {
        match patterns_to_xsd(&selector.names) {
            Some(pattern) => {
                facets.push(required(FacetKind::Attribute {
                    name: IdsValue::Restriction(Restriction::enumeration([
                        "Name".to_owned(),
                        "ObjectType".to_owned(),
                    ])),
                    value: Some(pattern),
                }));
                warnings.add(
                    "Name patterns are matched against Name and ObjectType only, not the type name.",
                );
            }
            None => warnings.add(format!(
                "Part '{part}': name patterns {:?} have no IDS equivalent and were left out.",
                selector.names
            )),
        }
    }
    if !selector.materials.is_empty() {
        match patterns_to_xsd(&selector.materials) {
            Some(pattern) => facets.push(required(FacetKind::Material {
                value: Some(pattern),
            })),
            None => warnings.add(format!(
                "Part '{part}': material patterns {:?} have no IDS equivalent and were left out.",
                selector.materials
            )),
        }
    }
    for filter in &selector.properties {
        if filter.pset.is_empty() {
            warnings.add(NO_PSET);
            continue;
        }
        facets.push(required(FacetKind::Property {
            property_set: IdsValue::Simple(filter.pset.clone()),
            base_name: IdsValue::Simple(filter.name.clone()),
            data_type: None,
            value: filter.equals.clone().map(IdsValue::Simple),
        }));
    }
    facets
}

/// IDS requires every property set that matches `propertySet` to hold the
/// property, so "any property set" cannot be written.
const NO_PSET: &str = "Properties without a property set (`pset`) were left out: IDS requires \
     every matching property set to contain the property, so \"in any property set\" cannot \
     be written. Add `pset` to such sources to export them.";

/// The requirement for an item: the first source IDS can express, merged
/// with the other sources of the same kind.
fn item_facet(
    item: &InformationItem,
    sources: &[Source],
    warnings: &mut Warnings,
) -> Option<Facet> {
    #[derive(PartialEq, Clone, Copy)]
    enum Kind {
        Property,
        Attribute,
        Classification,
        Material,
        System,
        Storey,
    }
    let kind_of = |s: &Source| {
        if s.property.is_some() {
            s.pset.is_some().then_some(Kind::Property)
        } else if s.attribute.is_some() {
            Some(Kind::Attribute)
        } else if s.classification {
            Some(Kind::Classification)
        } else if s.material {
            Some(Kind::Material)
        } else if s.system {
            Some(Kind::System)
        } else if s.storey {
            Some(Kind::Storey)
        } else {
            None
        }
    };
    if item
        .any
        .iter()
        .any(|s| s.property.is_some() && s.pset.is_none())
    {
        warnings.add(NO_PSET);
    }
    let Some(kind) = sources.iter().find_map(kind_of) else {
        warnings.add(format!(
            "Item '{}' has no source IDS can require (geometry, type name or properties \
             without a property set); it was left out.",
            item.name
        ));
        return None;
    };
    let dropped = sources.iter().filter(|s| kind_of(s) != Some(kind)).count();
    if dropped > 0 {
        warnings.add(format!(
            "Item '{}': IDS has no alternatives between kinds of information; {dropped} \
             alternative source(s) were left out.",
            item.name
        ));
    }
    let same: Vec<&Source> = item
        .any
        .iter()
        .filter(|s| kind_of(s) == Some(kind))
        .collect();
    let facet_kind = match kind {
        Kind::Property => {
            let names: Vec<String> = same.iter().filter_map(|s| s.property.clone()).collect();
            let sets: Vec<String> = same.iter().filter_map(|s| s.pset.clone()).collect();
            let base_name = name_value(&names, item, warnings)?;
            if same.len() > 1 {
                warnings.add(format!(
                    "Item '{}': in IDS every listed property set must contain one of the \
                     listed property names; the pairing of set and name is lost.",
                    item.name
                ));
            }
            let property_set = name_value(&sets, item, warnings)?;
            FacetKind::Property {
                property_set,
                base_name,
                data_type: None,
                value: None,
            }
        }
        Kind::Attribute => {
            let names: Vec<String> = same.iter().filter_map(|s| s.attribute.clone()).collect();
            FacetKind::Attribute {
                name: name_value(&names, item, warnings)?,
                value: None,
            }
        }
        Kind::Classification => FacetKind::Classification {
            system: None,
            value: None,
        },
        Kind::Material => FacetKind::Material { value: None },
        Kind::System => FacetKind::PartOf {
            relation: Some("IFCRELASSIGNSTOGROUP".to_owned()),
            entity: EntityFacet {
                name: IdsValue::Restriction(Restriction::enumeration(
                    [
                        "IFCSYSTEM",
                        "IFCDISTRIBUTIONSYSTEM",
                        "IFCDISTRIBUTIONCIRCUIT",
                        "IFCBUILDINGSYSTEM",
                    ]
                    .map(String::from),
                )),
                predefined_type: None,
            },
        },
        Kind::Storey => FacetKind::PartOf {
            relation: Some("IFCRELCONTAINEDINSPATIALSTRUCTURE".to_owned()),
            entity: EntityFacet {
                name: IdsValue::Simple("IFCBUILDINGSTOREY".to_owned()),
                predefined_type: None,
            },
        },
    };
    let instructions = if item.description.is_empty() {
        item.name.clone()
    } else {
        format!("{}: {}", item.name, item.description)
    };
    Some(Facet {
        instructions,
        ..required(facet_kind)
    })
}

/// A name from name patterns: the exact name for one anchored literal,
/// otherwise an XSD pattern.
fn name_value(
    patterns: &[String],
    item: &InformationItem,
    warnings: &mut Warnings,
) -> Option<IdsValue> {
    if let [one] = patterns
        && let Some(literal) = anchored_literal(one)
    {
        return Some(IdsValue::Simple(literal));
    }
    let value = patterns_to_xsd(patterns);
    if value.is_none() {
        warnings.add(format!(
            "Item '{}': the patterns {patterns:?} have no IDS equivalent; the item was left out.",
            item.name
        ));
    }
    value
}

/// `^Name$` -> `Name` when the pattern has no special characters.
fn anchored_literal(pattern: &str) -> Option<String> {
    let inner = pattern.strip_prefix('^')?.strip_suffix('$')?;
    let plain = inner
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ');
    plain.then(|| inner.to_owned())
}

/// Several OpenCheck patterns (any may match) as one IDS pattern restriction.
fn patterns_to_xsd(patterns: &[String]) -> Option<IdsValue> {
    let parts: Option<Vec<String>> = patterns.iter().map(|p| regex_to_xsd(p)).collect();
    let pattern = parts?.join("|");
    Restriction::pattern(&pattern)
        .ok()
        .map(IdsValue::Restriction)
}

/// Translates an OpenCheck pattern (a regular expression matched anywhere in
/// the text, ignoring case) to an XSD pattern (which matches the whole text
/// and respects case). Letters become `[aA]` classes, unanchored ends get
/// `.*`, and word boundaries are dropped. Returns None for constructs XSD
/// lacks, such as `(?...)` groups.
pub(crate) fn regex_to_xsd(pattern: &str) -> Option<String> {
    let mut body = pattern;
    let anchored_start = body.starts_with('^');
    if anchored_start {
        body = &body[1..];
    }
    let anchored_end = body.ends_with('$') && !body.ends_with("\\$");
    if anchored_end {
        body = &body[..body.len() - 1];
    }
    if body.contains("(?") {
        return None;
    }
    let mut out = String::new();
    let mut chars = body.chars().peekable();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let next = chars.next()?;
                match next {
                    // Word boundaries have no XSD form; the match gets looser.
                    'b' | 'B' if !in_class => {}
                    'A' | 'z' | 'Z' => return None,
                    _ => {
                        out.push('\\');
                        out.push(next);
                    }
                }
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                if chars.peek() == Some(&'^') {
                    out.push(chars.next().expect("peeked"));
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '^' | '$' if !in_class => return None,
            c if c.is_alphabetic() && has_other_case(c) => {
                let (lower, upper) = cases(c);
                if in_class {
                    // A range such as a-z needs its other case too.
                    if chars.peek() == Some(&'-') {
                        let mut ahead = chars.clone();
                        ahead.next();
                        if let Some(end) = ahead.peek().copied().filter(|e| e.is_alphabetic()) {
                            chars.next();
                            chars.next();
                            let (end_lower, end_upper) = cases(end);
                            out.push_str(&format!("{lower}-{end_lower}{upper}-{end_upper}"));
                            continue;
                        }
                    }
                    out.push(lower);
                    out.push(upper);
                } else {
                    out.push('[');
                    out.push(lower);
                    out.push(upper);
                    out.push(']');
                }
            }
            c => out.push(c),
        }
    }
    if in_class {
        return None;
    }
    let start = if anchored_start { "" } else { ".*" };
    let end = if anchored_end { "" } else { ".*" };
    let xsd = format!("{start}({out}){end}");
    compile_pattern(&xsd).ok().map(|_| xsd)
}

fn has_other_case(c: char) -> bool {
    let (lower, upper) = cases(c);
    lower != upper
}

fn cases(c: char) -> (char, char) {
    let lower = c.to_lowercase().next().unwrap_or(c);
    let upper = c.to_uppercase().next().unwrap_or(c);
    // Letters such as ß have no single-character upper case.
    if c.to_uppercase().count() != 1 || c.to_lowercase().count() != 1 {
        return (c, c);
    }
    (lower, upper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::builtin_standard;

    #[test]
    fn regex_translation_keeps_meaning_without_case() {
        let xsd = regex_to_xsd("^(CCS|CCI)?ClassCode$").unwrap();
        let re = compile_pattern(&xsd).unwrap();
        assert!(re.is_match("CCSClassCode"));
        assert!(re.is_match("classcode"));
        assert!(!re.is_match("CCSClassCodeVersion"));

        let re = compile_pattern(&regex_to_xsd("\\btræ").unwrap()).unwrap();
        assert!(re.is_match("Solitært TRÆ 4"));
        assert!(!re.is_match("Busk"));

        let re = compile_pattern(&regex_to_xsd("^Type ?(kode|id)$").unwrap()).unwrap();
        assert!(re.is_match("TypeID"));
        assert!(re.is_match("type kode"));

        let typeid = Facet {
            kind: FacetKind::Property {
                property_set: IdsValue::Restriction(Restriction::pattern(".*").unwrap()),
                base_name: IdsValue::Restriction(
                    Restriction::pattern(&regex_to_xsd("^Type ?(kode|id)$").unwrap()).unwrap(),
                ),
                data_type: None,
                value: None,
            },
            ..required(FacetKind::Material { value: None })
        };
        assert_eq!(typeid.label(), "/type ?(kode|id)/");

        let re = compile_pattern(&regex_to_xsd("[a-c]x").unwrap()).unwrap();
        assert!(re.is_match("..BX.."));
        assert!(regex_to_xsd("(?i)x").is_none());
    }

    #[test]
    fn class_names_include_subtypes() {
        let walls = class_names("IfcWall");
        assert!(walls.contains(&"IFCWALL".to_owned()));
        assert!(walls.contains(&"IFCWALLSTANDARDCASE".to_owned()));
        assert!(!walls.contains(&"IFCWALLTYPE".to_owned()));
        assert!(!class_names("IfcSpace").contains(&"IFCSPACEHEATER".to_owned()));
    }

    #[test]
    fn properties_need_a_property_set() {
        let standard = InformationStandard::from_toml(
            r#"
            [standard]
            id = "t"
            name = "T"
            levels = ["1"]
            default_level = "1"
            [[item]]
            id = "fire"
            name = "Fire rating"
            any = [{ property = "^FireRating$", pset = "^Pset_WallCommon$" }]
            [[item]]
            id = "width"
            name = "Width"
            any = [{ property = "^Width$" }, { attribute = "^OverallWidth$" }]
            [[item]]
            id = "code"
            name = "Code"
            any = [{ property = "^Code$" }]
            [[part]]
            id = "wall"
            name = "Walls"
            applies = [{ classes = ["IfcWall"] }]
            [part.levels]
            "1" = ["fire", "width", "code"]
        "#,
        )
        .unwrap();
        let export = standard_to_ids(&standard, &[]);
        let spec = &export.ids.specifications[0];
        let labels: Vec<String> = spec.requirements.iter().map(Facet::label).collect();
        // The property set is written; the property without one falls back to
        // the attribute; "Code" has nothing IDS can require.
        assert_eq!(labels, vec!["Pset_WallCommon.FireRating", "OverallWidth"]);
        assert!(export.warnings.iter().any(|w| w == NO_PSET));
    }

    #[test]
    fn builtin_standard_exports_and_reads_back() {
        let standard = builtin_standard("opencheck.information.lod-dk").unwrap();
        let export = standard_to_ids(standard, &["300".to_owned()]);
        assert!(!export.ids.specifications.is_empty());
        assert!(!export.warnings.is_empty());
        let xml = export.ids.to_xml();
        let again = Ids::from_xml(&xml).unwrap_or_else(|e| panic!("{e}\n{xml}"));
        assert_eq!(again.specifications.len(), export.ids.specifications.len());
        assert_eq!(again.info.milestone, "LOD 300");
        for (a, b) in again.specifications.iter().zip(&export.ids.specifications) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.requirements.len(), b.requirements.len());
            assert_eq!(a.applicability.len(), b.applicability.len());
        }
        // All levels at once.
        let all = standard_to_ids(standard, &[]);
        assert!(all.ids.specifications.len() > export.ids.specifications.len());
    }
}
