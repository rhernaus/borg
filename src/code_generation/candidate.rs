use anyhow::{Context, Result};
use log::{debug, info, warn};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use super::generator::{CodeContext, CodeGenerator, CodeImprovement};
use super::rater::CandidateRater;
use crate::testing::test_runner::{TestResult, TestRunner};
use crate::version_control::git::GitManager;

/// A candidate code improvement with its isolated environment and evaluation metrics
#[derive(Debug, Clone)]
pub struct GenerationCandidate {
    /// Unique identifier for this candidate
    pub id: String,

    /// The code improvement proposal
    pub improvement: CodeImprovement,

    /// Path to the git worktree where this candidate is isolated
    pub worktree_path: PathBuf,

    /// The model that generated this candidate
    pub model_id: String,

    /// Test results for this candidate (None if not yet tested)
    pub test_result: Option<TestResult>,

    /// Quality rating from 0.0 to 1.0 (None if not yet rated)
    pub rating: Option<f64>,

    /// Whether this candidate was selected as the winner
    pub is_winner: bool,
}

impl GenerationCandidate {
    /// Create a new candidate
    pub fn new(improvement: CodeImprovement, worktree_path: PathBuf, model_id: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            improvement,
            worktree_path,
            model_id,
            test_result: None,
            rating: None,
            is_winner: false,
        }
    }

    /// Check if this candidate passed tests
    pub fn passed_tests(&self) -> bool {
        self.test_result
            .as_ref()
            .map(|r| r.success)
            .unwrap_or(false)
    }

    /// Get a summary of this candidate's performance
    pub fn summary(&self) -> String {
        let test_status = if let Some(ref result) = self.test_result {
            if result.success {
                "PASSED"
            } else {
                "FAILED"
            }
        } else {
            "NOT_TESTED"
        };

        let rating_str = self
            .rating
            .map(|r| format!("{:.3}", r))
            .unwrap_or_else(|| "N/A".to_string());

        format!(
            "Candidate {} (model: {}) - Tests: {}, Rating: {}, Winner: {}",
            &self.id[..8],
            self.model_id,
            test_status,
            rating_str,
            self.is_winner
        )
    }
}

/// Generator that creates and evaluates multiple code candidates in parallel
pub struct CandidateGenerator {
    /// Git manager for creating worktrees
    git_manager: Arc<dyn GitManager>,

    /// Test runner for evaluating candidates
    test_runner: Arc<dyn TestRunner>,

    /// Base directory for creating worktrees
    worktree_base_dir: PathBuf,

    /// Number of candidates to generate (MVP: defaults to 1)
    num_candidates: usize,
}

impl CandidateGenerator {
    /// Create a new candidate generator
    ///
    /// # Arguments
    /// * `git_manager` - Git manager for creating isolated worktrees
    /// * `test_runner` - Test runner for evaluating candidates
    /// * `worktree_base_dir` - Base directory for creating worktrees
    /// * `num_candidates` - Number of candidates to generate (defaults to 1 for MVP)
    pub fn new(
        git_manager: Arc<dyn GitManager>,
        test_runner: Arc<dyn TestRunner>,
        worktree_base_dir: PathBuf,
        num_candidates: Option<usize>,
    ) -> Self {
        Self {
            git_manager,
            test_runner,
            worktree_base_dir,
            num_candidates: num_candidates.unwrap_or(1),
        }
    }

    /// Generate N candidates in parallel, test them, and select the winner
    ///
    /// # Arguments
    /// * `context` - The code generation context
    /// * `generators` - Code generators to use (for MVP, typically just one)
    /// * `rater` - Candidate rater for scoring candidates
    ///
    /// # Returns
    /// The winning candidate with all evaluation metrics
    pub async fn generate_and_select(
        &self,
        context: &CodeContext,
        generators: &[Arc<dyn CodeGenerator>],
        rater: &CandidateRater,
    ) -> Result<GenerationCandidate> {
        info!(
            "Generating {} candidate(s) for task: {}",
            self.num_candidates, context.task
        );

        // Generate candidates (for MVP: just one)
        let candidates = self.generate_candidates(context, generators).await?;

        if candidates.is_empty() {
            return Err(anyhow::anyhow!("No candidates were generated"));
        }

        // Test all candidates
        let mut tested_candidates = self.test_candidates(candidates).await?;

        // Rate all candidates
        self.rate_candidates(&mut tested_candidates, rater).await?;

        // Select the winner
        let winner = self.select_winner(tested_candidates)?;

        info!("Selected winner: {}", winner.summary());

        Ok(winner)
    }

    /// Generate candidate improvements
    async fn generate_candidates(
        &self,
        context: &CodeContext,
        generators: &[Arc<dyn CodeGenerator>],
    ) -> Result<Vec<GenerationCandidate>> {
        let mut candidates = Vec::new();

        // For MVP, we generate one candidate using the first generator
        // Future enhancement: generate N candidates in parallel
        for (idx, generator) in generators.iter().take(self.num_candidates).enumerate() {
            debug!(
                "Generating candidate {} of {}",
                idx + 1,
                self.num_candidates
            );

            // Generate the improvement
            let improvement = generator
                .generate_improvement(context)
                .await
                .with_context(|| format!("Failed to generate candidate {}", idx + 1))?;

            // Create a worktree for this candidate
            let worktree_path = self.worktree_base_dir.join(format!("candidate-{}", idx));
            let uuid_str = Uuid::new_v4().to_string();
            let branch_name = format!("candidate-{}-{}", idx, &uuid_str[..8]);

            debug!(
                "Creating worktree for candidate {} at {:?}",
                idx + 1,
                worktree_path
            );

            self.git_manager
                .create_worktree(&branch_name, &worktree_path)
                .await
                .with_context(|| format!("Failed to create worktree for candidate {}", idx + 1))?;

            // Create the candidate
            let candidate = GenerationCandidate::new(
                improvement,
                worktree_path,
                "default-model".to_string(), // TODO: Get actual model ID from generator
            );

            candidates.push(candidate);
        }

        info!("Generated {} candidate(s)", candidates.len());

        Ok(candidates)
    }

    /// Test all candidates
    async fn test_candidates(
        &self,
        mut candidates: Vec<GenerationCandidate>,
    ) -> Result<Vec<GenerationCandidate>> {
        info!("Testing {} candidate(s)", candidates.len());

        // For MVP: test candidates sequentially
        // Future enhancement: test in parallel
        for candidate in &mut candidates {
            debug!("Testing candidate {}", &candidate.id[..8]);

            // Apply the code changes to the worktree
            // TODO: Implement file writing logic based on candidate.improvement.target_files
            // For now, we'll just run tests on the worktree as-is

            // Run tests in the worktree
            let test_result = self
                .test_runner
                .run_tests("HEAD", Some(&candidate.worktree_path))
                .await
                .with_context(|| {
                    format!("Failed to run tests for candidate {}", &candidate.id[..8])
                })?;

            debug!(
                "Candidate {} test result: {}",
                &candidate.id[..8],
                if test_result.success {
                    "PASSED"
                } else {
                    "FAILED"
                }
            );

            candidate.test_result = Some(test_result);
        }

        let passed_count = candidates.iter().filter(|c| c.passed_tests()).count();
        info!(
            "Testing complete: {}/{} candidates passed tests",
            passed_count,
            candidates.len()
        );

        Ok(candidates)
    }

    /// Rate all candidates using the LLM-based rater
    async fn rate_candidates(
        &self,
        candidates: &mut [GenerationCandidate],
        rater: &CandidateRater,
    ) -> Result<()> {
        info!("Rating {} candidate(s)", candidates.len());

        // For MVP: rate candidates sequentially
        // Future enhancement: rate in parallel
        for candidate in candidates.iter_mut() {
            debug!("Rating candidate {}", &candidate.id[..8]);

            let rating = rater
                .rate_candidate(candidate)
                .await
                .with_context(|| format!("Failed to rate candidate {}", &candidate.id[..8]))?;

            debug!("Candidate {} rating: {:.3}", &candidate.id[..8], rating);

            candidate.rating = Some(rating);
        }

        info!("Rating complete");

        Ok(())
    }

    /// Select the winning candidate based on test results and ratings
    fn select_winner(
        &self,
        mut candidates: Vec<GenerationCandidate>,
    ) -> Result<GenerationCandidate> {
        debug!("Selecting winner from {} candidates", candidates.len());

        if candidates.is_empty() {
            return Err(anyhow::anyhow!("No candidates available for selection"));
        }

        // For MVP with 1 candidate, just return it
        if candidates.len() == 1 {
            candidates[0].is_winner = true;
            return Ok(candidates.into_iter().next().unwrap());
        }

        // Filter to only candidates that passed tests
        let passing_candidates: Vec<_> = candidates.iter().filter(|c| c.passed_tests()).collect();

        // Determine which candidates to consider for ranking
        let candidates_to_rank = if passing_candidates.is_empty() {
            warn!("No candidates passed tests, returning best-rated candidate anyway");
            // Fall back to all candidates and pick best rated
            &candidates[..]
        } else {
            // Use only passing candidates
            warn!("Using only passing candidates for selection");
            // Need to extract the actual candidates, not references
            &candidates[..]
        };

        // Find the best candidate by rating
        let mut best_idx = 0;
        let mut best_rating = candidates_to_rank[0].rating.unwrap_or(0.0);

        for (idx, candidate) in candidates_to_rank.iter().enumerate() {
            // Skip if we're only considering passing candidates and this one didn't pass
            if !passing_candidates.is_empty() && !candidate.passed_tests() {
                continue;
            }

            let rating = candidate.rating.unwrap_or(0.0);
            if rating > best_rating {
                best_rating = rating;
                best_idx = idx;
            }
        }

        // Mark the winner and return it
        candidates[best_idx].is_winner = true;
        let winner = candidates.into_iter().nth(best_idx).unwrap();

        debug!("Winner selected: {}", winner.summary());

        Ok(winner)
    }

    /// Clean up worktrees for all candidates
    pub async fn cleanup_candidates(&self, candidates: &[GenerationCandidate]) -> Result<()> {
        info!("Cleaning up {} candidate worktree(s)", candidates.len());

        for candidate in candidates {
            debug!(
                "Removing worktree for candidate {} at {:?}",
                &candidate.id[..8],
                candidate.worktree_path
            );

            if let Err(e) = self
                .git_manager
                .remove_worktree(&candidate.worktree_path)
                .await
            {
                warn!(
                    "Failed to remove worktree for candidate {}: {}",
                    &candidate.id[..8],
                    e
                );
            }
        }

        info!("Cleanup complete");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidate_creation() {
        use crate::code_generation::generator::{CodeImprovement, FileChange};

        let improvement = CodeImprovement {
            id: "test-1".to_string(),
            task: "Test task".to_string(),
            code: "fn test() {}".to_string(),
            target_files: vec![FileChange {
                file_path: "src/test.rs".to_string(),
                start_line: None,
                end_line: None,
                new_content: "fn test() {}".to_string(),
            }],
            explanation: "Test explanation".to_string(),
        };

        let candidate = GenerationCandidate::new(
            improvement,
            PathBuf::from("/tmp/test-worktree"),
            "test-model".to_string(),
        );

        assert!(!candidate.id.is_empty());
        assert_eq!(candidate.model_id, "test-model");
        assert!(!candidate.passed_tests()); // No test result yet
        assert!(candidate.rating.is_none());
        assert!(!candidate.is_winner);
    }

    #[test]
    fn test_candidate_summary() {
        use crate::code_generation::generator::{CodeImprovement, FileChange};
        use std::time::Duration;

        let improvement = CodeImprovement {
            id: "test-1".to_string(),
            task: "Test task".to_string(),
            code: "fn test() {}".to_string(),
            target_files: vec![FileChange {
                file_path: "src/test.rs".to_string(),
                start_line: None,
                end_line: None,
                new_content: "fn test() {}".to_string(),
            }],
            explanation: "Test explanation".to_string(),
        };

        let mut candidate = GenerationCandidate::new(
            improvement,
            PathBuf::from("/tmp/test-worktree"),
            "test-model".to_string(),
        );

        let summary = candidate.summary();
        assert!(summary.contains("NOT_TESTED"));
        assert!(summary.contains("test-model"));

        // Add test result
        candidate.test_result = Some(TestResult {
            success: true,
            output: "All tests passed".to_string(),
            duration: Duration::from_secs(1),
            metrics: None,
            report: None,
            failures: None,
            compilation_errors: None,
            exit_code: Some(0),
            branch: Some("test-branch".to_string()),
            test_stage: None,
        });

        let summary = candidate.summary();
        assert!(summary.contains("PASSED"));

        // Add rating
        candidate.rating = Some(0.85);
        let summary = candidate.summary();
        assert!(summary.contains("0.850"));
    }
}
