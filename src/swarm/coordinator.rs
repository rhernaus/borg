//! SwarmCoordinator - Main orchestration using config-driven phases.
//!
//! Architecture (Phase 3 restructure):
//! - Each phase (Research, Deliberation, TDD) has ONE prompt defined in config
//! - That prompt is run on MULTIPLE models (also from config)
//! - No agent/lens complexity - config drives everything

use anyhow::Result;
use log::{debug, error, info, warn};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::code_generation::llm::{LlmFactory, LlmProvider};
use crate::code_generation::llm_tool::{
    BashTool, CompilationFeedbackTool, EditTool, FindTestsTool, GitCommandTool, GitHistoryTool,
    GrepTool, ReadTool, TestRunnerTool, TodoWriteTool, ToolRegistry, WebFetchTool, WebSearchTool,
    WriteTool,
};
use crate::core::config::{Config, LlmConfig, LlmLoggingConfig, ModelConfig, PhaseConfig};
use crate::providers::ResponseFormat;
use crate::testing::test_runner::TestRunner;
use crate::version_control::git::GitManager;

use super::agent::Proposal;
use super::constitution::Constitution;
use super::council::ConsensusResult;
use super::telos::EudaimonicTelos;

/// Result of a complete swarm cycle
#[derive(Debug)]
pub enum SwarmCycleResult {
    /// Successfully executed an improvement
    Success {
        proposal: Proposal,
        changes_applied: bool,
        tests_passed: bool,
    },
    /// No proposals were approved by the council
    NoConsensus {
        proposals_count: usize,
        rejection_reasons: Vec<String>,
    },
    /// Execution failed
    ExecutionFailed { proposal: Proposal, error: String },
    /// No improvements identified
    NoImprovementsFound,
}

/// The SwarmCoordinator orchestrates the entire swarm cycle
pub struct SwarmCoordinator {
    telos: EudaimonicTelos,
    constitution: Arc<Constitution>,
    config: Config,
    approval_threshold: f64,
    git_manager: Arc<Mutex<dyn GitManager>>,
    test_runner: Arc<dyn TestRunner>,
}

impl SwarmCoordinator {
    /// Create a new swarm coordinator with config-driven architecture
    pub async fn new(
        config: Config,
        git_manager: Arc<Mutex<dyn GitManager>>,
        test_runner: Arc<dyn TestRunner>,
    ) -> Result<Self> {
        let telos = EudaimonicTelos::default();
        let constitution = Arc::new(Constitution::new());

        info!("Creating SwarmCoordinator with config-driven phases");
        info!("Research models: {:?}", config.phases.research.models);
        info!(
            "Deliberation models: {:?}",
            config.phases.deliberation.models
        );
        info!("TDD models: {:?}", config.phases.tdd.models);

        Ok(Self {
            telos,
            constitution,
            config,
            approval_threshold: 0.5,
            git_manager,
            test_runner,
        })
    }

    /// Create an LLM provider for a specific model config
    fn create_llm_for_model(
        model_config: &ModelConfig,
        log_dir: &str,
    ) -> Result<Box<dyn LlmProvider>> {
        // Convert ModelConfig to LlmConfig format for LlmFactory
        let llm_config = LlmConfig {
            provider: model_config.provider.clone(),
            api_key: model_config.api_key.clone().unwrap_or_default(),
            model: model_config.model.clone(),
            max_tokens: model_config.max_tokens,
            temperature: model_config.temperature,
            api_base: model_config.api_base.clone(),
            headers: None,
            enable_streaming: None,
            enable_thinking: model_config.enable_thinking,
            reasoning_effort: model_config.reasoning_effort.clone(),
            reasoning_budget_tokens: model_config.reasoning_budget_tokens,
            first_token_timeout_ms: None,
            stall_timeout_ms: None,
        };

        let llm_logging = LlmLoggingConfig {
            enabled: true,
            log_dir: log_dir.to_string(),
            console_logging: false,
            include_full_prompts: true,
            include_full_responses: true,
            max_log_size_mb: 100,
            log_files_to_keep: 10,
        };

        LlmFactory::create(llm_config, llm_logging)
    }

    /// Create a ToolRegistry filtered by the phase's allowed tools
    fn create_tool_registry(
        phase: &PhaseConfig,
        workspace: &Path,
        git_manager: Arc<Mutex<dyn GitManager>>,
    ) -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        let allowed_tools: std::collections::HashSet<&str> =
            phase.tools.iter().map(|s| s.as_str()).collect();

        // Register only the tools that are allowed for this phase
        // Read tools (supporting new names: Read, Grep, Glob and old names)
        if allowed_tools.contains("Read") || allowed_tools.contains("file_contents") {
            registry.register(ReadTool::new(workspace.to_path_buf()));
        }
        if allowed_tools.contains("Grep") || allowed_tools.contains("code_search") {
            registry.register(GrepTool::new(workspace.to_path_buf(), git_manager.clone()));
        }
        if allowed_tools.contains("Glob") || allowed_tools.contains("explore_dir") {
            registry.register(BashTool::new(workspace.to_path_buf()));
        }
        if allowed_tools.contains("find_tests") {
            registry.register(FindTestsTool::new(workspace.to_path_buf()));
        }
        if allowed_tools.contains("git_history") {
            registry.register(GitHistoryTool::new(
                workspace.to_path_buf(),
                git_manager.clone(),
            ));
        }
        if allowed_tools.contains("compile_check") {
            registry.register(CompilationFeedbackTool::new(workspace.to_path_buf()));
        }
        if allowed_tools.contains("run_tests") {
            registry.register(TestRunnerTool::new(workspace.to_path_buf()));
        }
        if allowed_tools.contains("WebSearch") || allowed_tools.contains("web_search") {
            registry.register(WebSearchTool::new());
        }
        if allowed_tools.contains("WebFetch") {
            registry.register(WebFetchTool::new());
        }

        // Write tools (supporting new names: Write, Edit, Bash and old names)
        if allowed_tools.contains("Write") || allowed_tools.contains("create_file") {
            registry.register(WriteTool::new(workspace.to_path_buf()));
        }
        if allowed_tools.contains("Edit") || allowed_tools.contains("modify_file") {
            registry.register(EditTool::new(workspace.to_path_buf()));
        }
        if allowed_tools.contains("Bash") || allowed_tools.contains("git_command") {
            registry.register(GitCommandTool::new(workspace.to_path_buf()));
        }

        // TodoWrite tool (coordinator-level tracking)
        if allowed_tools.contains("TodoWrite") {
            registry.register(TodoWriteTool::new());
        }

        // Note: Task tool is NOT added to the registry - it's coordinator-level only
        // to prevent recursion

        debug!(
            "Created tool registry with {} tools for phase",
            phase.tools.len()
        );
        registry
    }

    /// Run a single swarm cycle
    pub async fn run_cycle(&self, codebase_context: &str) -> Result<SwarmCycleResult> {
        info!("Starting swarm cycle");
        info!("Telos: {}", self.telos.purpose);

        // Phase 1: Research - run prompt on all research models
        info!("Phase 1: Research");
        let proposals = self.research_phase(codebase_context).await?;

        if proposals.is_empty() {
            warn!("No proposals generated");
            return Ok(SwarmCycleResult::NoImprovementsFound);
        }

        info!("Received {} proposals", proposals.len());

        // Phase 2: Deliberation - score proposals using multiple models
        info!("Phase 2: Deliberation");
        let consensus = self.deliberation_phase(proposals.clone()).await?;

        let approved_proposal = match consensus {
            Some(ConsensusResult::Approved {
                proposal,
                geometric_mean,
                ..
            }) => {
                info!(
                    "Proposal '{}' approved with score {:.2}",
                    proposal.title, geometric_mean
                );
                proposal
            }
            Some(ConsensusResult::Rejected {
                proposal, reason, ..
            }) => {
                let reason_str = format!("{:?}", reason);
                warn!("Proposal '{}' rejected: {}", proposal.title, reason_str);
                return Ok(SwarmCycleResult::NoConsensus {
                    proposals_count: proposals.len(),
                    rejection_reasons: vec![reason_str],
                });
            }
            None => {
                warn!("No consensus reached on any proposal");
                return Ok(SwarmCycleResult::NoConsensus {
                    proposals_count: proposals.len(),
                    rejection_reasons: vec!["No proposals approved".into()],
                });
            }
        };

        // Phase 3: Execution - TDD loop
        info!("Phase 3: Execution");
        let execution_result = self
            .execution_phase(&approved_proposal, codebase_context)
            .await;

        match execution_result {
            Ok((changes_applied, tests_passed)) => {
                info!(
                    "Execution complete: changes={}, tests={}",
                    changes_applied, tests_passed
                );
                Ok(SwarmCycleResult::Success {
                    proposal: approved_proposal,
                    changes_applied,
                    tests_passed,
                })
            }
            Err(e) => {
                error!("Execution failed: {}", e);
                Ok(SwarmCycleResult::ExecutionFailed {
                    proposal: approved_proposal,
                    error: e.to_string(),
                })
            }
        }
    }

    /// Phase 1: Research - run research prompt on all configured models
    async fn research_phase(&self, codebase_context: &str) -> Result<Vec<Proposal>> {
        let phase = &self.config.phases.research;
        info!(
            "Research phase using {} models, {} tools available",
            phase.models.len(),
            phase.tools.len()
        );

        if !phase.tools.is_empty() {
            debug!("Available tools: {:?}", phase.tools);
            // Create tool registry for this phase
            let workspace = PathBuf::from(&self.config.agent.working_dir);
            let _tool_registry =
                Self::create_tool_registry(phase, &workspace, self.git_manager.clone());
            // TODO: Wire tool_registry into the LLM conversation loop
            // This requires multi-turn conversation support with tool calls
        }

        let mut proposals = Vec::new();
        let prompt_template = &phase.prompt;

        // Substitute {{context}} placeholder
        let prompt = prompt_template.replace("{{context}}", codebase_context);

        // Run the same prompt on all research models
        let mut futures = Vec::new();
        for model_name in &phase.models {
            if let Some(model_config) = self.config.get_model(model_name) {
                let model_config = model_config.clone();
                let log_dir = self.config.logging.llm_log_dir.clone();
                let prompt = prompt.clone();
                let constitution = self.constitution.clone();
                let model_name = model_name.clone();

                futures.push(async move {
                    Self::run_research_on_model(
                        &model_name,
                        &model_config,
                        &log_dir,
                        &prompt,
                        &constitution,
                    )
                    .await
                });
            } else {
                warn!("Model '{}' not found in config", model_name);
            }
        }

        // Run all in parallel
        let results = futures::future::join_all(futures).await;

        for (i, result) in results.into_iter().enumerate() {
            let model_name = &self.config.phases.research.models[i];
            match result {
                Ok(proposal) => {
                    info!("Model '{}' proposed: '{}'", model_name, proposal.title);
                    proposals.push(proposal);
                }
                Err(e) => {
                    warn!("Model '{}' failed to generate proposal: {}", model_name, e);
                }
            }
        }

        Ok(proposals)
    }

    /// Run research on a single model
    async fn run_research_on_model(
        model_name: &str,
        model_config: &ModelConfig,
        log_dir: &str,
        prompt: &str,
        constitution: &Constitution,
    ) -> Result<Proposal> {
        let llm = Self::create_llm_for_model(model_config, log_dir)?;

        // Use JSON mode to ensure valid JSON responses
        let response = llm
            .generate_with_format(
                prompt,
                Some(16384),
                None,
                Some(ResponseFormat::json_object()),
            )
            .await?;

        // Extract JSON (handles markdown code block wrapping from some models)
        let json_str = extract_json_from_response(&response);

        // Parse JSON response
        let parsed: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse LLM response: {}", e))?;

        let proposal = Proposal {
            id: format!("proposal-{}-{}", model_name, uuid::Uuid::new_v4()),
            agent_id: format!("model-{}", model_name),
            title: parsed["title"].as_str().unwrap_or("Untitled").into(),
            description: parsed["description"].as_str().unwrap_or("").into(),
            rationale: parsed["rationale"].as_str().unwrap_or("").into(),
            files_to_modify: parsed["files_to_modify"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            files_to_create: parsed["files_to_create"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            files_to_delete: parsed["files_to_delete"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            estimated_lines_changed: parsed["estimated_lines_changed"].as_u64().unwrap_or(0)
                as usize,
            expected_benefits: parsed["expected_benefits"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            potential_risks: parsed["potential_risks"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        };

        // Validate against constitution before returning
        let action = proposal.to_proposed_action();
        if let Err(violation) = constitution.validate(&action) {
            return Err(anyhow::anyhow!(
                "Proposal violates constitution [{:?}]: {}",
                violation.priority,
                violation.description
            ));
        }

        Ok(proposal)
    }

    /// Phase 2: Deliberation - score each proposal using all deliberation models
    async fn deliberation_phase(
        &self,
        proposals: Vec<Proposal>,
    ) -> Result<Option<ConsensusResult>> {
        if proposals.is_empty() {
            return Ok(None);
        }

        info!(
            "Deliberation phase: {} proposals, {} models per proposal",
            proposals.len(),
            self.config.phases.deliberation.models.len()
        );

        // For each proposal, score it with all deliberation models
        let mut best_proposal: Option<ConsensusResult> = None;
        let mut best_score = 0.0;

        for proposal in proposals {
            let scores = self.score_proposal(&proposal).await?;

            if scores.is_empty() {
                warn!("No scores for proposal '{}'", proposal.title);
                continue;
            }

            // Check for vetoes (score = 0.0)
            let vetoes: Vec<String> = scores
                .iter()
                .filter(|(_, score)| *score == 0.0)
                .map(|(model, _)| model.clone())
                .collect();

            if !vetoes.is_empty() {
                info!(
                    "Proposal '{}' vetoed by: {}",
                    proposal.title,
                    vetoes.join(", ")
                );
                continue;
            }

            // Calculate geometric mean
            let geometric_mean =
                calculate_geometric_mean(&scores.iter().map(|(_, s)| *s).collect::<Vec<_>>());

            info!("Proposal '{}' score: {:.2}", proposal.title, geometric_mean);

            if geometric_mean >= self.approval_threshold && geometric_mean > best_score {
                best_score = geometric_mean;
                best_proposal = Some(ConsensusResult::Approved {
                    proposal: proposal.clone(),
                    geometric_mean,
                    votes: vec![], // Simplified - no detailed votes in this architecture
                });
            }
        }

        Ok(best_proposal)
    }

    /// Score a single proposal using all deliberation models
    async fn score_proposal(&self, proposal: &Proposal) -> Result<Vec<(String, f64)>> {
        let prompt_template = &self.config.phases.deliberation.prompt;

        // Serialize the proposal for the prompt
        let proposal_json = serde_json::to_string_pretty(proposal)?;
        let prompt = prompt_template.replace("{{proposal}}", &proposal_json);

        let mut futures = Vec::new();
        for model_name in &self.config.phases.deliberation.models {
            if let Some(model_config) = self.config.get_model(model_name) {
                let model_config = model_config.clone();
                let log_dir = self.config.logging.llm_log_dir.clone();
                let prompt = prompt.clone();
                let model_name = model_name.clone();

                futures.push(async move {
                    Self::run_deliberation_on_model(&model_name, &model_config, &log_dir, &prompt)
                        .await
                });
            }
        }

        let results = futures::future::join_all(futures).await;

        let mut scores = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            let model_name = &self.config.phases.deliberation.models[i];
            match result {
                Ok(score) => {
                    info!("Model '{}' scored: {:.2}", model_name, score);
                    scores.push((model_name.clone(), score));
                }
                Err(e) => {
                    warn!("Model '{}' failed to score: {}", model_name, e);
                }
            }
        }

        Ok(scores)
    }

    /// Run deliberation on a single model
    async fn run_deliberation_on_model(
        _model_name: &str,
        model_config: &ModelConfig,
        log_dir: &str,
        prompt: &str,
    ) -> Result<f64> {
        let llm = Self::create_llm_for_model(model_config, log_dir)?;

        let response = llm
            .generate_with_format(
                prompt,
                Some(16384),
                None,
                Some(ResponseFormat::json_object()),
            )
            .await?;

        let json_str = extract_json_from_response(&response);
        let parsed: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse LLM response: {}", e))?;

        let score = parsed["score"].as_f64().unwrap_or(0.0);
        Ok(score)
    }

    /// Phase 3: Execute the approved proposal via TDD using multiple models
    async fn execution_phase(
        &self,
        proposal: &Proposal,
        _codebase_context: &str,
    ) -> Result<(bool, bool)> {
        info!(
            "Execution phase using {} models",
            self.config.phases.tdd.models.len()
        );

        // Validate against constitution one more time
        let action = proposal.to_proposed_action();
        if let Err(violation) = self.constitution.validate(&action) {
            return Err(anyhow::anyhow!(
                "Constitutional violation [{:?}]: {}",
                violation.priority,
                violation.description
            ));
        }

        // Create a branch for this work
        let branch_name = format!("swarm/{}", proposal.id);

        {
            let git = self.git_manager.lock().await;
            // Create branch for the work
            if let Err(e) = git.create_branch(&branch_name).await {
                warn!("Could not create branch {}: {}", branch_name, e);
            }
        }

        // TODO: Implement actual TDD execution using config.phases.tdd
        // The TDD models will implement the proposal via Test-Driven Development
        info!("TDD execution placeholder - proposal: {}", proposal.title);
        info!("Files to modify: {:?}", proposal.files_to_modify);
        info!("Files to create: {:?}", proposal.files_to_create);

        // Run tests to verify current state
        let test_result = self.test_runner.run_tests(&branch_name, None).await?;

        Ok((false, test_result.success))
    }

    /// Run the continuous improvement loop
    pub async fn run(
        &self,
        codebase_context: &str,
        max_cycles: Option<usize>,
    ) -> Result<Vec<SwarmCycleResult>> {
        let max_cycles = max_cycles.unwrap_or(usize::MAX);
        let mut results = Vec::new();

        for cycle in 0..max_cycles {
            info!("=== Swarm Cycle {} ===", cycle + 1);

            let result = self.run_cycle(codebase_context).await?;

            let should_continue = matches!(result, SwarmCycleResult::Success { .. });

            results.push(result);

            if !should_continue {
                info!("Stopping swarm loop");
                break;
            }
        }

        Ok(results)
    }
}

/// Extract JSON from a response that may be wrapped in markdown code blocks.
fn extract_json_from_response(response: &str) -> &str {
    use regex::Regex;
    use std::sync::OnceLock;

    static JSON_BLOCK_RE: OnceLock<Regex> = OnceLock::new();
    let re =
        JSON_BLOCK_RE.get_or_init(|| Regex::new(r"```(?:json)?\s*\n?([\s\S]*?)\n?```").unwrap());

    if let Some(captures) = re.captures(response) {
        if let Some(json_match) = captures.get(1) {
            return json_match.as_str().trim();
        }
    }

    response.trim()
}

/// Calculate geometric mean of scores
fn calculate_geometric_mean(scores: &[f64]) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }

    // Use log sum to avoid overflow/underflow
    let log_sum: f64 = scores
        .iter()
        .map(|&score| {
            // Clamp scores to avoid log(0)
            let score = score.clamp(0.001, 1.0);
            score.ln()
        })
        .sum();

    let n = scores.len() as f64;
    (log_sum / n).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometric_mean() {
        let scores = vec![0.8, 0.8];
        let mean = calculate_geometric_mean(&scores);
        assert!((mean - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_geometric_mean_with_low_score() {
        let scores = vec![0.9, 0.1];
        let mean = calculate_geometric_mean(&scores);
        // sqrt(0.9 * 0.1) = sqrt(0.09) ≈ 0.3
        assert!(mean < 0.35);
        assert!(mean > 0.25);
    }
}
