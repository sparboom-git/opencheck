//! Danish practice for consistency and clash checking (C402): issue
//! criticality C1–C3, discipline labels and the issue type.

use opencheck_core::Discipline;
use opencheck_core::check::Severity;

/// The issue type C402 uses for findings from consistency and clash checks.
pub const ISSUE_TYPE: &str = "K&K-kontrol";

/// Criticality code of an issue priority: C1 not critical, C2 critical,
/// C3 very critical.
pub fn code(priority: Severity) -> &'static str {
    match priority {
        Severity::Low => "C1",
        Severity::Moderate => "C2",
        Severity::Critical => "C3",
    }
}

/// Code and meaning, e.g. "C3 – meget kritisk".
pub fn label(priority: Severity) -> &'static str {
    match priority {
        Severity::Low => "C1 – ikke-kritisk",
        Severity::Moderate => "C2 – kritisk",
        Severity::Critical => "C3 – meget kritisk",
    }
}

/// When to use each category, shown as help.
pub fn explanation(priority: Severity) -> &'static str {
    match priority {
        Severity::Low => "No effect on quality, cost or time, e.g. agreed or theoretical overlaps.",
        Severity::Moderate => "Affects quality, cost or time, buildability or construction.",
        Severity::Critical => {
            "Large effect on quality, cost or time; must be handled before other issues."
        }
    }
}

/// Most critical first, as they are offered.
pub const LEVELS: [Severity; 3] = [Severity::Critical, Severity::Moderate, Severity::Low];

/// The discipline label used to group issues ("label: fag").
pub fn discipline_label(discipline: Discipline) -> Option<&'static str> {
    Some(match discipline {
        Discipline::Architectural => "Arkitektur",
        Discipline::Landscape => "Landskab",
        Discipline::Structural => "Konstruktion",
        Discipline::Electrical => "El",
        Discipline::Plumbing => "VVS",
        Discipline::Mechanical => "Ventilation",
        Discipline::FireProtection => "Sprinkling",
        Discipline::Infrastructure => "Forsyning",
        Discipline::Unknown => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn criticality_follows_priority() {
        assert_eq!(code(Severity::Critical), "C3");
        assert_eq!(code(Severity::Low), "C1");
        assert_eq!(LEVELS.map(code), ["C3", "C2", "C1"]);
        assert_eq!(
            discipline_label(Discipline::Mechanical),
            Some("Ventilation")
        );
        assert_eq!(discipline_label(Discipline::Unknown), None);
    }
}
