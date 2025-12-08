//! Constitutional constraints with lexicographic ordering.
//!
//! Priority hierarchy (higher priority violations reject regardless of lower priority scores):
//! P1: Corrigibility - system can always be stopped/corrected
//! P2: Safety - don't break things, preserve system integrity
//! P3: Low Impact (AUP) - minimal changes, preserve optionality
//! P4: Eudaimonic Task - genuine value, not slop

use serde::{Deserialize, Serialize};

/// Constitutional priority levels (lexicographically ordered)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConstitutionalPriority {
    /// P1: Corrigibility - can always be stopped
    Corrigibility = 1,
    /// P2: Safety - don't break things
    Safety = 2,
    /// P3: Low Impact - minimal changes (AUP)
    LowImpact = 3,
    /// P4: Eudaimonic Task - genuine value
    EudaimonicTask = 4,
}

/// A constraint violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintViolation {
    pub priority: ConstitutionalPriority,
    pub constraint_name: String,
    pub description: String,
    pub severity: f64, // 0.0 = none, 1.0 = complete violation
}

/// A proposed action to be validated
#[derive(Debug, Clone)]
pub struct ProposedAction {
    pub description: String,
    pub files_to_modify: Vec<String>,
    pub files_to_create: Vec<String>,
    pub files_to_delete: Vec<String>,
    pub estimated_lines_changed: usize,
}

/// The Constitution - validates actions against lexicographic constraints
pub struct Constitution {
    /// Protected paths that should never be modified (corrigibility)
    protected_paths: Vec<String>,
    /// Maximum files that can be changed in one action (low impact)
    max_files_per_change: usize,
    /// Maximum lines that can be changed (low impact)
    max_lines_per_change: usize,
    /// Patterns that indicate dangerous operations (safety)
    danger_patterns: Vec<String>,
}

impl Default for Constitution {
    fn default() -> Self {
        Self {
            protected_paths: vec![
                "src/swarm/constitution.rs".into(), // Don't modify the constitution
                ".git/".into(),
                "Cargo.lock".into(),
            ],
            max_files_per_change: 10,
            max_lines_per_change: 500,
            danger_patterns: vec![
                "rm -rf".into(),
                "sudo".into(),
                "chmod 777".into(),
                "DROP TABLE".into(),
                "DELETE FROM".into(),
                "format!(\"{}\", user_input)".into(), // SQL injection risk
            ],
        }
    }
}

impl Constitution {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate an action against all constitutional constraints.
    /// Returns Ok(()) if valid, Err with the highest-priority violation if invalid.
    /// Lexicographic ordering: P1 violations checked first, then P2, etc.
    pub fn validate(&self, action: &ProposedAction) -> Result<(), ConstraintViolation> {
        // P1: Corrigibility checks
        self.check_corrigibility(action)?;

        // P2: Safety checks
        self.check_safety(action)?;

        // P3: Low Impact (AUP) checks
        self.check_low_impact(action)?;

        // P4: Eudaimonic checks (basic - full check needs LLM)
        self.check_eudaimonic(action)?;

        Ok(())
    }

    /// P1: Corrigibility - ensure the action doesn't block shutdown or correction
    fn check_corrigibility(&self, action: &ProposedAction) -> Result<(), ConstraintViolation> {
        // Check if any protected paths are being modified
        for path in action
            .files_to_modify
            .iter()
            .chain(action.files_to_delete.iter())
        {
            for protected in &self.protected_paths {
                if path.contains(protected) {
                    return Err(ConstraintViolation {
                        priority: ConstitutionalPriority::Corrigibility,
                        constraint_name: "protected_path".into(),
                        description: format!(
                            "Cannot modify protected path '{}' - this could compromise corrigibility",
                            path
                        ),
                        severity: 1.0,
                    });
                }
            }
        }

        // Check for patterns that could disable logging/monitoring
        let disable_patterns = [
            "disable logging",
            "disable_logging",
            "skip audit",
            "skip_audit",
            "bypass check",
            "bypass_check",
        ];
        for pattern in disable_patterns {
            if action.description.to_lowercase().contains(pattern) {
                return Err(ConstraintViolation {
                    priority: ConstitutionalPriority::Corrigibility,
                    constraint_name: "disable_monitoring".into(),
                    description: format!(
                        "Action appears to disable monitoring/logging: '{}'",
                        pattern
                    ),
                    severity: 1.0,
                });
            }
        }

        Ok(())
    }

    /// P2: Safety - ensure the action doesn't break things
    fn check_safety(&self, action: &ProposedAction) -> Result<(), ConstraintViolation> {
        // Check for dangerous patterns
        for pattern in &self.danger_patterns {
            if action.description.contains(pattern) {
                return Err(ConstraintViolation {
                    priority: ConstitutionalPriority::Safety,
                    constraint_name: "danger_pattern".into(),
                    description: format!("Action contains dangerous pattern: '{}'", pattern),
                    severity: 1.0,
                });
            }
        }

        // Check for mass deletions
        if action.files_to_delete.len() > 5 {
            return Err(ConstraintViolation {
                priority: ConstitutionalPriority::Safety,
                constraint_name: "mass_deletion".into(),
                description: format!(
                    "Action deletes {} files - this exceeds safe limit of 5",
                    action.files_to_delete.len()
                ),
                severity: 0.8,
            });
        }

        Ok(())
    }

    /// P3: Low Impact (AUP) - minimize changes, preserve optionality
    fn check_low_impact(&self, action: &ProposedAction) -> Result<(), ConstraintViolation> {
        let total_files = action.files_to_modify.len()
            + action.files_to_create.len()
            + action.files_to_delete.len();

        if total_files > self.max_files_per_change {
            return Err(ConstraintViolation {
                priority: ConstitutionalPriority::LowImpact,
                constraint_name: "too_many_files".into(),
                description: format!(
                    "Action affects {} files - exceeds limit of {}",
                    total_files, self.max_files_per_change
                ),
                severity: 0.6,
            });
        }

        if action.estimated_lines_changed > self.max_lines_per_change {
            return Err(ConstraintViolation {
                priority: ConstitutionalPriority::LowImpact,
                constraint_name: "too_many_lines".into(),
                description: format!(
                    "Action changes {} lines - exceeds limit of {}",
                    action.estimated_lines_changed, self.max_lines_per_change
                ),
                severity: 0.5,
            });
        }

        Ok(())
    }

    /// P4: Eudaimonic - ensure genuine value (basic check)
    fn check_eudaimonic(&self, action: &ProposedAction) -> Result<(), ConstraintViolation> {
        // Basic slop detection - empty or trivial changes
        if action.description.trim().is_empty() {
            return Err(ConstraintViolation {
                priority: ConstitutionalPriority::EudaimonicTask,
                constraint_name: "empty_description".into(),
                description: "Action has no description - cannot verify value".into(),
                severity: 1.0,
            });
        }

        // Detect trivial changes
        let trivial_patterns = ["add comment", "fix typo", "rename variable", "format code"];

        let desc_lower = action.description.to_lowercase();
        for pattern in trivial_patterns {
            if desc_lower.contains(pattern) && action.estimated_lines_changed > 50 {
                return Err(ConstraintViolation {
                    priority: ConstitutionalPriority::EudaimonicTask,
                    constraint_name: "trivial_large_change".into(),
                    description: format!(
                        "Action claims '{}' but changes {} lines - suspicious",
                        pattern, action.estimated_lines_changed
                    ),
                    severity: 0.7,
                });
            }
        }

        Ok(())
    }

    /// Score an action's constitutional compliance (0.0 = violation, 1.0 = perfect)
    pub fn score(&self, action: &ProposedAction) -> f64 {
        match self.validate(action) {
            Ok(()) => 1.0,
            Err(violation) => 1.0 - violation.severity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constitution_allows_valid_action() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Add new feature for user authentication".into(),
            files_to_modify: vec!["src/auth.rs".into()],
            files_to_create: vec!["src/auth/oauth.rs".into()],
            files_to_delete: vec![],
            estimated_lines_changed: 100,
        };
        assert!(constitution.validate(&action).is_ok());
    }

    #[test]
    fn test_constitution_blocks_protected_path() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Modify constitution".into(),
            files_to_modify: vec!["src/swarm/constitution.rs".into()],
            files_to_create: vec![],
            files_to_delete: vec![],
            estimated_lines_changed: 10,
        };
        let result = constitution.validate(&action);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().priority,
            ConstitutionalPriority::Corrigibility
        );
    }

    #[test]
    fn test_constitution_blocks_dangerous_pattern() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Clean up with rm -rf temp/".into(),
            files_to_modify: vec![],
            files_to_create: vec![],
            files_to_delete: vec!["temp/file.rs".into()],
            estimated_lines_changed: 0,
        };
        let result = constitution.validate(&action);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().priority, ConstitutionalPriority::Safety);
    }

    #[test]
    fn test_constitution_blocks_too_many_files() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Refactor entire codebase".into(),
            files_to_modify: vec![
                "src/a.rs".into(),
                "src/b.rs".into(),
                "src/c.rs".into(),
                "src/d.rs".into(),
                "src/e.rs".into(),
                "src/f.rs".into(),
                "src/g.rs".into(),
                "src/h.rs".into(),
                "src/i.rs".into(),
                "src/j.rs".into(),
                "src/k.rs".into(),
            ],
            files_to_create: vec![],
            files_to_delete: vec![],
            estimated_lines_changed: 100,
        };
        let result = constitution.validate(&action);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().priority,
            ConstitutionalPriority::LowImpact
        );
    }

    #[test]
    fn test_constitution_blocks_too_many_lines() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Add massive feature".into(),
            files_to_modify: vec!["src/lib.rs".into()],
            files_to_create: vec![],
            files_to_delete: vec![],
            estimated_lines_changed: 1000,
        };
        let result = constitution.validate(&action);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().priority,
            ConstitutionalPriority::LowImpact
        );
    }

    #[test]
    fn test_constitution_blocks_empty_description() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "".into(),
            files_to_modify: vec!["src/lib.rs".into()],
            files_to_create: vec![],
            files_to_delete: vec![],
            estimated_lines_changed: 10,
        };
        let result = constitution.validate(&action);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().priority,
            ConstitutionalPriority::EudaimonicTask
        );
    }

    #[test]
    fn test_constitution_blocks_trivial_large_change() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Fix typo in variable name".into(),
            files_to_modify: vec!["src/lib.rs".into()],
            files_to_create: vec![],
            files_to_delete: vec![],
            estimated_lines_changed: 100,
        };
        let result = constitution.validate(&action);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().priority,
            ConstitutionalPriority::EudaimonicTask
        );
    }

    #[test]
    fn test_constitution_blocks_disable_monitoring() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Disable logging to improve performance".into(),
            files_to_modify: vec!["src/lib.rs".into()],
            files_to_create: vec![],
            files_to_delete: vec![],
            estimated_lines_changed: 10,
        };
        let result = constitution.validate(&action);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().priority,
            ConstitutionalPriority::Corrigibility
        );
    }

    #[test]
    fn test_constitution_blocks_mass_deletion() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Remove old test files".into(),
            files_to_modify: vec![],
            files_to_create: vec![],
            files_to_delete: vec![
                "test1.rs".into(),
                "test2.rs".into(),
                "test3.rs".into(),
                "test4.rs".into(),
                "test5.rs".into(),
                "test6.rs".into(),
            ],
            estimated_lines_changed: 0,
        };
        let result = constitution.validate(&action);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().priority, ConstitutionalPriority::Safety);
    }

    #[test]
    fn test_constitution_score_valid() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Add feature".into(),
            files_to_modify: vec!["src/lib.rs".into()],
            files_to_create: vec![],
            files_to_delete: vec![],
            estimated_lines_changed: 50,
        };
        assert_eq!(constitution.score(&action), 1.0);
    }

    #[test]
    fn test_constitution_score_violation() {
        let constitution = Constitution::default();
        let action = ProposedAction {
            description: "Clean up with rm -rf temp/".into(),
            files_to_modify: vec![],
            files_to_create: vec![],
            files_to_delete: vec![],
            estimated_lines_changed: 0,
        };
        assert_eq!(constitution.score(&action), 0.0); // severity 1.0 -> score 0.0
    }
}
