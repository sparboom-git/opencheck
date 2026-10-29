//! Suggested C402 categories for findings, from the pre-sorting or from
//! Claude, and what happens when the reviewer approves them: C1 findings
//! need no action and are marked not a problem, C2 and C3 findings become
//! issues with that priority. A suggestion approved as another category is
//! kept in the project's triage file, so the next review learns from it.

use opencheck_core::project;
use opencheck_core::triage::{self, Category, Lookup, Rating, Source, TriageExample};
use rustc_hash::FxHashSet;

use crate::app::{IssueTarget, OpenCheckApp};

impl OpenCheckApp {
    /// Runs the pre-sorting on the current results: for the clash rules
    /// that have it switched on, or for all clash rules with `all` (the
    /// Pre-sort button). Earlier pre-sorting suggestions are replaced;
    /// Claude's, approved ones and decided findings are left alone.
    /// Returns how many findings got a suggestion.
    pub fn presort(&mut self, all: bool) -> usize {
        let Some(report) = self.checking.report.clone() else {
            return 0;
        };
        let before = self.checking.ratings.len();
        self.checking
            .ratings
            .retain(|_, r| !(r.source == Source::Presort && r.is_proposed()));
        let skip: FxHashSet<String> = self
            .checking
            .ratings
            .keys()
            .chain(self.checking.decisions.keys())
            .cloned()
            .collect();
        let current: triage::PresortSettings = self
            .checking
            .rulesets
            .iter()
            .flat_map(|r| {
                r.ruleset.rules.iter().map(|rule| {
                    (
                        (r.ruleset.ruleset.id.clone(), rule.id.clone()),
                        rule.presort.clone(),
                    )
                })
            })
            .collect();
        let found = triage::presort(
            &report,
            &self.federation,
            &current,
            all,
            &skip,
            &project::now_rfc3339(),
        );
        let count = found.len();
        self.checking.ratings.extend(found);
        if count > 0 || self.checking.ratings.len() != before {
            self.session.dirty = true;
        }
        count
    }

    /// Approves the suggestions for `ids`, as suggested or as `category`.
    pub fn approve_ratings(&mut self, ids: &[String], category: Option<Category>) {
        let lookup_examples: Vec<TriageExample> = {
            let lookup = Lookup::new(&self.federation);
            ids.iter()
                .filter_map(|id| {
                    let rating = self.checking.ratings.get(id)?;
                    let chosen = category?;
                    if !rating.is_proposed() || chosen == rating.category {
                        return None;
                    }
                    let (_, _, finding) = self.checking.find(id)?;
                    Some(TriageExample {
                        finding: lookup.describe(finding),
                        suggested: rating.category,
                        category: chosen,
                        reason: rating.reason.clone(),
                    })
                })
                .collect()
        };
        self.checking.triage.examples.extend(lookup_examples);

        let mut changed = false;
        for id in ids {
            let Some((rating, chosen)) = self.checking.approve_rating(id, category) else {
                continue;
            };
            changed = true;
            // A C1 needs no action; a C2 or C3 becomes an issue with that
            // priority, framed on the finding (nobody is looking at it).
            if matches!(chosen, Category::C2 | Category::C3)
                && self.issues.for_finding(id).is_none()
                && let Some(guid) =
                    self.add_issues(std::slice::from_ref(id), IssueTarget::EachFramed)
                && let Some(issue) = self.issues.get_mut(&guid)
            {
                issue.priority = chosen.severity();
                issue.description = issue_note(&rating, chosen);
            }
        }
        if changed {
            self.session.dirty = true;
        }
    }

    /// Removes the suggestions for `ids`: the findings stand without one.
    pub fn dismiss_ratings(&mut self, ids: &[String]) {
        if self.checking.dismiss_ratings(ids) > 0 {
            self.session.dirty = true;
        }
    }
}

/// The issue description for an approved suggestion.
fn issue_note(rating: &Rating, chosen: Category) -> String {
    let by = rating.source.label();
    let confidence = (rating.confidence * 100.0).round();
    if chosen == rating.category {
        format!(
            "{}\n\n{} suggested by {by} ({confidence:.0} % sure).",
            rating.reason,
            chosen.code()
        )
    } else {
        format!(
            "{}\n\nSuggested as {} by {by} ({confidence:.0} % sure); approved as {}.",
            rating.reason,
            rating.category.code(),
            chosen.code()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rating(category: Category) -> Rating {
        Rating {
            category,
            noise: false,
            confidence: 0.8,
            reason: "Kanal gennem bjælke uden udsparing.".to_owned(),
            source: Source::Claude,
            approved: None,
            group: String::new(),
            image: false,
            at: String::new(),
        }
    }

    #[test]
    fn issue_notes_carry_the_reason_and_who_suggested_what() {
        let as_suggested = issue_note(&rating(Category::C2), Category::C2);
        assert!(as_suggested.starts_with("Kanal gennem bjælke uden udsparing."));
        assert!(as_suggested.contains("C2 suggested by Claude (80 % sure)."));

        let corrected = issue_note(&rating(Category::C2), Category::C3);
        assert!(corrected.contains("Suggested as C2 by Claude (80 % sure); approved as C3."));
    }
}
