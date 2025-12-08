//! The intrinsic telos - Eudaimonic Utility Function

use serde::{Deserialize, Serialize};

/// Dimensions of human flourishing (from FAI Benchmark)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlourishingDimension {
    CharacterAndVirtue,
    Relationships,
    Health,
    Finances,
    Meaning,
    Happiness,
    Spirituality,
}

/// The intrinsic purpose of the swarm - always active
#[derive(Debug, Clone)]
pub struct EudaimonicTelos {
    pub purpose: String,
    pub dimensions: Vec<FlourishingDimension>,
}

impl Default for EudaimonicTelos {
    fn default() -> Self {
        Self {
            purpose: "Maximize human flourishing subject to constitutional constraints".into(),
            dimensions: vec![
                FlourishingDimension::CharacterAndVirtue,
                FlourishingDimension::Relationships,
                FlourishingDimension::Health,
                FlourishingDimension::Meaning,
                FlourishingDimension::Happiness,
            ],
        }
    }
}

impl EudaimonicTelos {
    /// Generate a prompt that guides the swarm toward flourishing-aligned improvements
    pub fn generate_research_prompt(&self, codebase_context: &str) -> String {
        format!(
            r#"You are part of an autonomous swarm with an intrinsic purpose: {}

Your task is to identify improvements to this codebase that would best serve human flourishing.

Consider these dimensions of flourishing:
- Character & Virtue: Does this promote ethical behavior and integrity?
- Relationships: Does this facilitate genuine human connection?
- Health: Does this support human well-being?
- Meaning: Does this help humans understand their purpose?
- Happiness: Does this contribute to long-term life satisfaction?

Codebase context:
{}

Propose an improvement that genuinely advances human flourishing, not just superficial metrics."#,
            self.purpose, codebase_context
        )
    }
}
