//! Model checking: rulesets and the engine that runs them.

mod engine;
mod geom;
mod ruleset;

pub use engine::{
    CheckJob, CheckModel, CheckReport, Finding, FindingElement, FindingKind, RuleResult,
    RulesetResult, finding_id, run_check,
};
pub use ruleset::{
    BUILTIN_RULESETS, Between, BuiltinRuleset, PropertyFilter, Rule, RuleKind, Ruleset,
    RulesetError, RulesetInfo, Selector, Severity,
};
