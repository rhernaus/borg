use anyhow::{Context, Result};
use log::{debug, info};
use std::sync::Arc;

use super::candidate::GenerationCandidate;
use super::llm::LlmProvider;

/// Rates code candidates using an LLM to assess quality
pub struct CandidateRater {
    /// LLM provider for rating candidates
    llm_provider: Arc<dyn LlmProvider>,

    /// Temperature for LLM calls (lower = more deterministic)
    temperature: f32,

    /// Max tokens for rating responses
    max_tokens: usize,
}

impl CandidateRater {
    /// Create a new candidate rater
    ///
    /// # Arguments
    /// * `llm_provider` - LLM provider to use for rating
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            llm_provider,
            temperature: 0.3, // Lower temperature for more consistent ratings
            max_tokens: 500,
        }
    }

    /// Create a new candidate rater with custom parameters
    ///
    /// # Arguments
    /// * `llm_provider` - LLM provider to use for rating
    /// * `temperature` - Temperature for LLM calls
    /// * `max_tokens` - Max tokens for rating responses
    pub fn new_with_params(
        llm_provider: Arc<dyn LlmProvider>,
        temperature: f32,
        max_tokens: usize,
    ) -> Self {
        Self {
            llm_provider,
            temperature,
            max_tokens,
        }
    }

    /// Rate a single candidate's code quality
    ///
    /// Returns a score from 0.0 (worst) to 1.0 (best)
    pub async fn rate_candidate(&self, candidate: &GenerationCandidate) -> Result<f64> {
        debug!(
            "Rating candidate {} (model: {})",
            &candidate.id[..8],
            candidate.model_id
        );

        let prompt = self.build_rating_prompt(candidate);

        let response = self
            .llm_provider
            .generate(&prompt, Some(self.max_tokens), Some(self.temperature))
            .await
            .context("Failed to get rating from LLM")?;

        let rating = self
            .parse_rating(&response)
            .context("Failed to parse rating from LLM response")?;

        debug!("Candidate {} rated: {:.3}", &candidate.id[..8], rating);

        Ok(rating)
    }

    /// Rank multiple candidates by quality
    ///
    /// Returns candidates sorted by rating (highest first)
    pub async fn rank_candidates(
        &self,
        candidates: &[GenerationCandidate],
    ) -> Result<Vec<(usize, f64)>> {
        info!("Ranking {} candidates", candidates.len());

        let mut ratings = Vec::new();

        for (idx, candidate) in candidates.iter().enumerate() {
            let rating = self
                .rate_candidate(candidate)
                .await
                .with_context(|| format!("Failed to rate candidate {}", idx))?;

            ratings.push((idx, rating));
        }

        // Sort by rating (highest first)
        ratings.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        info!("Ranking complete");

        Ok(ratings)
    }

    /// Build the rating prompt for a candidate
    fn build_rating_prompt(&self, candidate: &GenerationCandidate) -> String {
        let test_status = if let Some(ref result) = candidate.test_result {
            if result.success {
                format!("PASSED (duration: {:?})", result.duration)
            } else {
                format!(
                    "FAILED\n\nTest output:\n{}",
                    result
                        .output
                        .lines()
                        .take(20)
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        } else {
            "NOT_TESTED".to_string()
        };

        format!(
            r#"You are a code quality expert. Please rate the following code improvement on a scale from 0.0 to 1.0.

Consider these factors:
- Code correctness and functionality
- Code quality, readability, and maintainability
- Adherence to Rust best practices
- Test results (if available)
- Whether the code achieves the stated task

TASK:
{task}

EXPLANATION:
{explanation}

CODE:
{code}

TEST STATUS:
{test_status}

Please provide your rating as a single decimal number between 0.0 and 1.0, followed by a brief explanation.
Format your response as:
RATING: <number>
EXPLANATION: <your explanation>

For example:
RATING: 0.85
EXPLANATION: The code is well-structured and passes all tests, but could benefit from additional error handling."#,
            task = candidate.improvement.task,
            explanation = candidate.improvement.explanation,
            code = candidate.improvement.code,
            test_status = test_status
        )
    }

    /// Parse the rating from the LLM response
    fn parse_rating(&self, response: &str) -> Result<f64> {
        // Look for "RATING: <number>" pattern
        for line in response.lines() {
            let trimmed = line.trim();
            if trimmed.to_uppercase().starts_with("RATING:") {
                // Extract the number after "RATING:"
                let rating_str = trimmed[7..].trim();

                // Try to parse as f64
                if let Ok(rating) = rating_str.parse::<f64>() {
                    // Clamp to [0.0, 1.0]
                    let clamped = rating.clamp(0.0, 1.0);
                    return Ok(clamped);
                }
            }
        }

        // Fallback: try to find any number in the response
        let numbers: Vec<f64> = response
            .split_whitespace()
            .filter_map(|word| {
                // Remove common punctuation
                let cleaned = word.trim_matches(|c: char| !c.is_numeric() && c != '.');
                cleaned.parse::<f64>().ok()
            })
            .collect();

        // Find the first number in a reasonable range [0.0, 1.0]
        for num in numbers {
            if (0.0..=1.0).contains(&num) {
                debug!("Parsed rating from fallback method: {}", num);
                return Ok(num);
            }
        }

        // If we still can't find a rating, return a default middle value
        debug!(
            "Could not parse rating from response, using default 0.5. Response: {}",
            response
        );
        Ok(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rating_standard_format() {
        let rater = CandidateRater::new(Arc::new(MockLlmProvider));

        let response = "RATING: 0.85\nEXPLANATION: The code is good";
        let rating = rater.parse_rating(response).unwrap();
        assert!((rating - 0.85).abs() < 0.001);
    }

    #[test]
    fn test_parse_rating_case_insensitive() {
        let rater = CandidateRater::new(Arc::new(MockLlmProvider));

        let response = "rating: 0.75\nexplanation: works well";
        let rating = rater.parse_rating(response).unwrap();
        assert!((rating - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_parse_rating_clamping() {
        let rater = CandidateRater::new(Arc::new(MockLlmProvider));

        // Test upper bound clamping
        let response = "RATING: 1.5\nEXPLANATION: too high";
        let rating = rater.parse_rating(response).unwrap();
        assert!((rating - 1.0).abs() < 0.001);

        // Test lower bound clamping
        let response = "RATING: -0.3\nEXPLANATION: too low";
        let rating = rater.parse_rating(response).unwrap();
        assert!(rating.abs() < 0.001);
    }

    #[test]
    fn test_parse_rating_fallback() {
        let rater = CandidateRater::new(Arc::new(MockLlmProvider));

        // No RATING: prefix, should find the number
        let response = "I would rate this 0.6 out of 1.0";
        let rating = rater.parse_rating(response).unwrap();
        assert!((rating - 0.6).abs() < 0.001);
    }

    #[test]
    fn test_parse_rating_default() {
        let rater = CandidateRater::new(Arc::new(MockLlmProvider));

        // No parseable rating, should default to 0.5
        let response = "This is a good improvement";
        let rating = rater.parse_rating(response).unwrap();
        assert!((rating - 0.5).abs() < 0.001);
    }

    // Mock LLM provider for testing
    struct MockLlmProvider;

    #[async_trait::async_trait]
    impl LlmProvider for MockLlmProvider {
        async fn generate(
            &self,
            _prompt: &str,
            _max_tokens: Option<usize>,
            _temperature: Option<f32>,
        ) -> Result<String> {
            Ok("RATING: 0.8\nEXPLANATION: Mock response".to_string())
        }

        async fn generate_streaming(
            &self,
            _prompt: &str,
            _max_tokens: Option<usize>,
            _temperature: Option<f32>,
            _print_tokens: bool,
        ) -> Result<String> {
            Ok("RATING: 0.8\nEXPLANATION: Mock response".to_string())
        }
    }
}
