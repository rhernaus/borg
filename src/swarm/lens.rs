//! Agent lenses - heterogeneous perspectives to prevent monoculture

use serde::{Deserialize, Serialize};

/// Category of lens perspective
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LensCategory {
    /// FAI dimensions - used in Research phase for evaluating human flourishing
    Flourishing,
    /// Code perspectives - used in TDD phase for implementation quality
    Code,
}

/// A perspective or "lens" that colors an agent's analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLens {
    pub id: String,
    pub name: String,
    pub role_description: String,
    pub priorities: Vec<String>,
    pub system_prompt_modifier: String,
    /// Model to use for this lens (e.g., "anthropic/claude-opus-4.5")
    /// Different models provide different perspectives
    pub model: Option<String>,
    /// Category of this lens (Code or Flourishing)
    pub category: LensCategory,
}

/// Code-focused lenses for TDD phase implementation quality
/// Each lens uses a randomly assigned model to ensure diverse perspectives
pub fn code_lenses() -> Vec<AgentLens> {
    vec![
        AgentLens {
            id: "architect".into(),
            name: "System Architect".into(),
            role_description: "Focuses on clean architecture, modularity, and long-term maintainability".into(),
            priorities: vec![
                "separation of concerns".into(),
                "extensibility".into(),
                "clear interfaces".into(),
            ],
            system_prompt_modifier: "You prioritize architectural elegance and long-term system health over quick fixes. Consider how changes affect the overall system structure.".into(),
            model: None, // Will be randomly assigned
            category: LensCategory::Code,
        },
        AgentLens {
            id: "pragmatist".into(),
            name: "Pragmatic Engineer".into(),
            role_description: "Focuses on practical solutions, shipping quickly, minimal complexity".into(),
            priorities: vec![
                "simplicity".into(),
                "deliverability".into(),
                "minimum viable solution".into(),
            ],
            system_prompt_modifier: "You prefer simple, working solutions over elegant but complex ones. What is the smallest change that achieves the goal?".into(),
            model: None, // Will be randomly assigned
            category: LensCategory::Code,
        },
        AgentLens {
            id: "critic".into(),
            name: "Critical Reviewer".into(),
            role_description: "Seeks potential issues, edge cases, failure modes".into(),
            priorities: vec![
                "error handling".into(),
                "edge cases".into(),
                "defensive programming".into(),
            ],
            system_prompt_modifier: "You actively look for what could go wrong. What are the failure modes? What edge cases are missing? Where could this break?".into(),
            model: None, // Will be randomly assigned
            category: LensCategory::Code,
        },
        AgentLens {
            id: "security".into(),
            name: "Security Analyst".into(),
            role_description: "Evaluates security implications, attack surfaces, data protection".into(),
            priorities: vec![
                "data safety".into(),
                "minimal attack surface".into(),
                "principle of least privilege".into(),
            ],
            system_prompt_modifier: "You analyze all proposals through a security lens. What are the risks? How could this be exploited? What data is exposed?".into(),
            model: None, // Will be randomly assigned
            category: LensCategory::Code,
        },
    ]
}

/// All 7 FAI Benchmark dimensions for Research phase
pub fn flourishing_lenses() -> Vec<AgentLens> {
    vec![
        AgentLens {
            id: "character".into(),
            name: "Character & Virtue".into(),
            role_description: "Promotes ethical behavior, integrity, and self-regulation".into(),
            priorities: vec!["ethical behavior".into(), "integrity".into(), "self-regulation".into()],
            system_prompt_modifier: "You evaluate proposals through the lens of character development and virtue ethics. Does this promote ethical behavior and integrity? Consider how it affects self-regulation and virtue.".into(),
            model: None, // Will be randomly assigned
            category: LensCategory::Flourishing,
        },
        AgentLens {
            id: "relationships".into(),
            name: "Relationships".into(),
            role_description: "Fosters deep, authentic connections with other humans".into(),
            priorities: vec!["authentic connection".into(), "collaboration".into(), "community".into()],
            system_prompt_modifier: "You evaluate proposals through the lens of human relationships. Does this facilitate genuine human connection? Does it support collaboration and community?".into(),
            model: None,
            category: LensCategory::Flourishing,
        },
        AgentLens {
            id: "health".into(),
            name: "Health".into(),
            role_description: "Supports physical and mental well-being, resilience".into(),
            priorities: vec!["well-being".into(), "mental health".into(), "resilience".into()],
            system_prompt_modifier: "You evaluate proposals through the lens of human health and well-being. Does this support mental health, reduce stress, and build resilience?".into(),
            model: None,
            category: LensCategory::Flourishing,
        },
        AgentLens {
            id: "finances".into(),
            name: "Finances".into(),
            role_description: "Ensures material security and reduces financial stress".into(),
            priorities: vec!["efficiency".into(), "resource optimization".into(), "sustainability".into()],
            system_prompt_modifier: "You evaluate proposals through the lens of resource efficiency. Does this optimize resources? Is it sustainable and cost-effective?".into(),
            model: None,
            category: LensCategory::Flourishing,
        },
        AgentLens {
            id: "meaning".into(),
            name: "Meaning".into(),
            role_description: "Helps understand purpose and supports worthwhile activities".into(),
            priorities: vec!["purpose".into(), "worthwhile activities".into(), "deep work".into()],
            system_prompt_modifier: "You evaluate proposals through the lens of meaning and purpose. Does this help humans understand their purpose? Does it support worthwhile activities and deep work?".into(),
            model: None,
            category: LensCategory::Flourishing,
        },
        AgentLens {
            id: "happiness".into(),
            name: "Happiness".into(),
            role_description: "Contributes to long-term life satisfaction (eudaimonic)".into(),
            priorities: vec!["life satisfaction".into(), "eudaimonic happiness".into(), "long-term well-being".into()],
            system_prompt_modifier: "You evaluate proposals through the lens of happiness and life satisfaction. Does this contribute to eudaimonic happiness and long-term well-being, not just momentary pleasure?".into(),
            model: None,
            category: LensCategory::Flourishing,
        },
        AgentLens {
            id: "transcendence".into(),
            name: "Transcendence".into(),
            role_description: "Respects deeply held values and higher purpose".into(),
            priorities: vec!["higher purpose".into(), "values alignment".into(), "legacy".into()],
            system_prompt_modifier: "You evaluate proposals through the lens of transcendent values. Does this align with higher purpose? Does it contribute to lasting positive impact and legacy?".into(),
            model: None,
            category: LensCategory::Flourishing,
        },
    ]
}
