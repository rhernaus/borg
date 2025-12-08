//! Swarm agents with heterogeneous lenses.

use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::constitution::{Constitution, ProposedAction};
use super::lens::AgentLens;
use super::telos::EudaimonicTelos;
use crate::code_generation::llm::LlmProvider;
use crate::providers::ResponseFormat;

/// Extract JSON from a response that may be wrapped in markdown code blocks.
/// Some models (especially Claude via OpenRouter) wrap JSON in ```json ... ``` blocks
/// even when response_format is set to json_object.
fn extract_json_from_response(response: &str) -> &str {
    // First, try to find JSON in a markdown code block
    static JSON_BLOCK_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re =
        JSON_BLOCK_RE.get_or_init(|| Regex::new(r"```(?:json)?\s*\n?([\s\S]*?)\n?```").unwrap());

    if let Some(captures) = re.captures(response) {
        if let Some(json_match) = captures.get(1) {
            return json_match.as_str().trim();
        }
    }

    // If no code block found, return the original (trimmed)
    response.trim()
}

/// A proposal from a swarm agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub description: String,
    pub rationale: String,
    pub files_to_modify: Vec<String>,
    pub files_to_create: Vec<String>,
    pub files_to_delete: Vec<String>,
    pub estimated_lines_changed: usize,
    pub expected_benefits: Vec<String>,
    pub potential_risks: Vec<String>,
}

impl Proposal {
    /// Convert to ProposedAction for constitutional validation
    pub fn to_proposed_action(&self) -> ProposedAction {
        ProposedAction {
            description: format!("{}: {}", self.title, self.description),
            files_to_modify: self.files_to_modify.clone(),
            files_to_create: self.files_to_create.clone(),
            files_to_delete: self.files_to_delete.clone(),
            estimated_lines_changed: self.estimated_lines_changed,
        }
    }
}

/// Analysis result from an agent evaluating a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalAnalysis {
    pub agent_id: String,
    pub proposal_id: String,
    pub score: f64, // 0.0 to 1.0, 0.0 = veto
    pub is_veto: bool,
    pub rationale: String,
    pub concerns: Vec<String>,
    pub suggestions: Vec<String>,
}

/// Trait for swarm agents
#[async_trait]
pub trait SwarmAgent: Send + Sync {
    /// Get the agent's unique identifier
    fn id(&self) -> &str;

    /// Get the agent's lens (perspective)
    fn lens(&self) -> &AgentLens;

    /// Research and propose improvements based on the telos
    async fn research(&self, telos: &EudaimonicTelos, codebase_context: &str) -> Result<Proposal>;

    /// Analyze and score another agent's proposal
    async fn analyze_proposal(
        &self,
        proposal: &Proposal,
        telos: &EudaimonicTelos,
    ) -> Result<ProposalAnalysis>;
}

/// LLM-based swarm agent implementation
pub struct LlmSwarmAgent {
    id: String,
    lens: AgentLens,
    llm: Arc<dyn LlmProvider>,
    constitution: Arc<Constitution>,
}

impl LlmSwarmAgent {
    pub fn new(
        lens: AgentLens,
        llm: Arc<dyn LlmProvider>,
        constitution: Arc<Constitution>,
    ) -> Self {
        let id = format!("agent-{}", lens.id);
        Self {
            id,
            lens,
            llm,
            constitution,
        }
    }

    fn build_research_prompt(&self, telos: &EudaimonicTelos, context: &str) -> String {
        format!(
            r#"{system_modifier}

{telos_prompt}

Your specific perspective as {role}:
- Focus: {description}
- Priorities: {priorities}

Analyze the codebase and propose ONE specific improvement that:
1. Aligns with the eudaimonic telos (human flourishing)
2. Respects constitutional constraints (corrigibility, safety, low impact)
3. Reflects your unique perspective as {role}

Respond in JSON format:
{{
    "title": "Brief title of the improvement",
    "description": "Detailed description of what to change",
    "rationale": "Why this improves human flourishing",
    "files_to_modify": ["path/to/file.rs"],
    "files_to_create": ["path/to/new.rs"],
    "files_to_delete": [],
    "estimated_lines_changed": 50,
    "expected_benefits": ["benefit1", "benefit2"],
    "potential_risks": ["risk1"]
}}"#,
            system_modifier = self.lens.system_prompt_modifier,
            telos_prompt = telos.generate_research_prompt(context),
            role = self.lens.name,
            description = self.lens.role_description,
            priorities = self.lens.priorities.join(", "),
        )
    }

    fn build_analysis_prompt(&self, proposal: &Proposal, telos: &EudaimonicTelos) -> String {
        format!(
            r#"{system_modifier}

You are evaluating a proposal from another agent. Your role is {role}.

The intrinsic telos is: {purpose}

PROPOSAL TO EVALUATE:
Title: {title}
Description: {description}
Rationale: {rationale}
Files affected: {files}
Estimated changes: {lines} lines

Expected benefits: {benefits}
Potential risks: {risks}

Evaluate this proposal through your lens ({role}):
- Does it align with human flourishing?
- Does it respect constitutional constraints?
- What concerns do you have from your perspective?

Respond in JSON format:
{{
    "score": 0.0-1.0,  // 0.0 = veto, 1.0 = full approval
    "is_veto": false,  // true if you believe this should NOT proceed
    "rationale": "Your reasoning",
    "concerns": ["concern1", "concern2"],
    "suggestions": ["suggestion1"]
}}"#,
            system_modifier = self.lens.system_prompt_modifier,
            role = self.lens.name,
            purpose = telos.purpose,
            title = proposal.title,
            description = proposal.description,
            rationale = proposal.rationale,
            files = proposal.files_to_modify.join(", "),
            lines = proposal.estimated_lines_changed,
            benefits = proposal.expected_benefits.join(", "),
            risks = proposal.potential_risks.join(", "),
        )
    }
}

#[async_trait]
impl SwarmAgent for LlmSwarmAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn lens(&self) -> &AgentLens {
        &self.lens
    }

    async fn research(&self, telos: &EudaimonicTelos, codebase_context: &str) -> Result<Proposal> {
        let prompt = self.build_research_prompt(telos, codebase_context);

        // Use JSON mode to ensure valid JSON responses
        let response = self
            .llm
            .generate_with_format(
                &prompt,
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
            id: format!("proposal-{}-{}", self.lens.id, uuid::Uuid::new_v4()),
            agent_id: self.id.clone(),
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
        if let Err(violation) = self.constitution.validate(&action) {
            return Err(anyhow::anyhow!(
                "Proposal violates constitution [{:?}]: {}",
                violation.priority,
                violation.description
            ));
        }

        Ok(proposal)
    }

    async fn analyze_proposal(
        &self,
        proposal: &Proposal,
        telos: &EudaimonicTelos,
    ) -> Result<ProposalAnalysis> {
        let prompt = self.build_analysis_prompt(proposal, telos);

        // Use JSON mode to ensure valid JSON responses
        let response = self
            .llm
            .generate_with_format(
                &prompt,
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

        Ok(ProposalAnalysis {
            agent_id: self.id.clone(),
            proposal_id: proposal.id.clone(),
            score: parsed["score"].as_f64().unwrap_or(0.5),
            is_veto: parsed["is_veto"].as_bool().unwrap_or(false),
            rationale: parsed["rationale"].as_str().unwrap_or("").into(),
            concerns: parsed["concerns"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            suggestions: parsed["suggestions"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}
