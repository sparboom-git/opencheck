//! Model checking: rulesets and the engine that runs them.

mod engine;
pub mod freezone;
mod geom;
mod ids;
mod ids_export;
mod index;
mod information;
mod ruleset;
mod script;

pub use engine::{
    CheckJob, CheckModel, CheckReport, Finding, FindingElement, FindingKind, RuleResult,
    RulesetResult, finding_id, run_check,
};
pub use ids::{
    Cardinality, EntityFacet, Facet, FacetKind, Failure, IDS_PREFIX, Ids, IdsContext, IdsError,
    IdsInfo, IdsValue, Restriction, Specification, compile_pattern, ids_path,
};
pub use ids_export::{IdsExport, standard_to_ids};
pub use information::{
    BUILTIN_STANDARDS, BuildingPart, BuiltinStandard, InformationItem, InformationStandard,
    InformationSummary, ItemCoverage, ModelFacts, PartCoverage, Source, StandardInfo,
    StandardOverrides, builtin_standard, display_pattern, exact_name, exact_pattern, is_checked,
    validate_sources,
};
pub use ruleset::{
    BUILTIN_RULESETS, Between, BuiltinRuleset, PropertyFilter, Rule, RuleKind, Ruleset,
    RulesetError, RulesetInfo, RunDirection, Selector, Severity, Zone, ZoneSide,
    is_work_in_progress,
};
pub use script::{SCRIPT_API, compile as compile_script};
