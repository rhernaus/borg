//! Council for multi-agent deliberation with geometric mean consensus.
//!
//! Key principle: One veto (score=0.0) kills the proposal entirely.
//! This enforces that all dimensions of the swarm must approve.

use anyhow::Result;
use log::{info, warn};
use std::sync::Arc;

use super::agent::{Proposal, ProposalAnalysis, SwarmAgent};
use super::telos::EudaimonicTelos;

/// Result of council deliberation
#[derive(Debug, Clone)]
pub enum ConsensusResult {
    /// Proposal approved with aggregate score
    Approved {
        proposal: Proposal,
        geometric_mean: f64,
        votes: Vec<ProposalAnalysis>,
    },
    /// Proposal rejected (vetoed or below threshold)
    Rejected {
        proposal: Proposal,
        reason: RejectionReason,
        votes: Vec<ProposalAnalysis>,
    },
}

/// Why a proposal was rejected
#[derive(Debug, Clone)]
pub enum RejectionReason {
    /// At least one agent vetoed (score = 0.0)
    Vetoed { vetoing_agents: Vec<String> },
    /// Geometric mean below threshold
    BelowThreshold { score: f64, threshold: f64 },
    /// No proposals received
    NoProposals,
}

/// The Council coordinates agent deliberation
pub struct Council {
    agents: Vec<Arc<dyn SwarmAgent>>,
    telos: EudaimonicTelos,
    /// Minimum geometric mean score to approve (default: 0.5)
    approval_threshold: f64,
}

impl Council {
    pub fn new(agents: Vec<Arc<dyn SwarmAgent>>, telos: EudaimonicTelos) -> Self {
        Self {
            agents,
            telos,
            approval_threshold: 0.5,
        }
    }

    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.approval_threshold = threshold;
        self
    }

    /// Deliberate on a single proposal
    pub async fn deliberate_proposal(&self, proposal: &Proposal) -> Result<ConsensusResult> {
        let mut votes = Vec::new();

        // Collect votes from all agents
        for agent in &self.agents {
            // Skip the proposing agent
            if agent.id() == proposal.agent_id {
                continue;
            }

            match agent.analyze_proposal(proposal, &self.telos).await {
                Ok(analysis) => {
                    info!(
                        "Agent {} scored proposal {}: {:.2} (veto: {})",
                        agent.id(),
                        proposal.id,
                        analysis.score,
                        analysis.is_veto
                    );
                    votes.push(analysis);
                }
                Err(e) => {
                    warn!("Agent {} failed to analyze proposal: {}", agent.id(), e);
                    // Treat analysis failure as abstention (not counted)
                }
            }
        }

        if votes.is_empty() {
            return Ok(ConsensusResult::Rejected {
                proposal: proposal.clone(),
                reason: RejectionReason::NoProposals,
                votes,
            });
        }

        // Check for vetoes first
        let vetoing_agents: Vec<String> = votes
            .iter()
            .filter(|v| v.is_veto || v.score == 0.0)
            .map(|v| v.agent_id.clone())
            .collect();

        if !vetoing_agents.is_empty() {
            return Ok(ConsensusResult::Rejected {
                proposal: proposal.clone(),
                reason: RejectionReason::Vetoed { vetoing_agents },
                votes,
            });
        }

        // Calculate geometric mean
        let geometric_mean = self.calculate_geometric_mean(&votes);

        if geometric_mean < self.approval_threshold {
            return Ok(ConsensusResult::Rejected {
                proposal: proposal.clone(),
                reason: RejectionReason::BelowThreshold {
                    score: geometric_mean,
                    threshold: self.approval_threshold,
                },
                votes,
            });
        }

        Ok(ConsensusResult::Approved {
            proposal: proposal.clone(),
            geometric_mean,
            votes,
        })
    }

    /// Deliberate on multiple proposals, return the best approved one
    pub async fn deliberate(&self, proposals: Vec<Proposal>) -> Result<Option<ConsensusResult>> {
        if proposals.is_empty() {
            return Ok(None);
        }

        let mut approved: Vec<ConsensusResult> = Vec::new();
        let mut rejected: Vec<ConsensusResult> = Vec::new();

        for proposal in proposals {
            let result = self.deliberate_proposal(&proposal).await?;
            match &result {
                ConsensusResult::Approved { geometric_mean, .. } => {
                    info!(
                        "Proposal {} approved with score {:.2}",
                        proposal.id, geometric_mean
                    );
                    approved.push(result);
                }
                ConsensusResult::Rejected { reason, .. } => {
                    info!("Proposal {} rejected: {:?}", proposal.id, reason);
                    rejected.push(result);
                }
            }
        }

        // Return the highest-scoring approved proposal
        if approved.is_empty() {
            return Ok(None);
        }

        approved.sort_by(|a, b| {
            let score_a = match a {
                ConsensusResult::Approved { geometric_mean, .. } => *geometric_mean,
                _ => 0.0,
            };
            let score_b = match b {
                ConsensusResult::Approved { geometric_mean, .. } => *geometric_mean,
                _ => 0.0,
            };
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(approved.into_iter().next())
    }

    /// Calculate geometric mean of scores
    /// Formula: (s1 * s2 * ... * sn)^(1/n)
    fn calculate_geometric_mean(&self, votes: &[ProposalAnalysis]) -> f64 {
        if votes.is_empty() {
            return 0.0;
        }

        // Use log sum to avoid overflow/underflow
        let log_sum: f64 = votes
            .iter()
            .map(|v| {
                // Clamp scores to avoid log(0)
                let score = v.score.clamp(0.001, 1.0);
                score.ln()
            })
            .sum();

        let n = votes.len() as f64;
        (log_sum / n).exp()
    }

    /// Get a summary of the deliberation
    pub fn summarize_result(&self, result: &ConsensusResult) -> String {
        match result {
            ConsensusResult::Approved {
                proposal,
                geometric_mean,
                votes,
            } => {
                let vote_summary: Vec<String> = votes
                    .iter()
                    .map(|v| format!("{}: {:.2}", v.agent_id, v.score))
                    .collect();
                format!(
                    "APPROVED: '{}'\nScore: {:.2}\nVotes: {}\n",
                    proposal.title,
                    geometric_mean,
                    vote_summary.join(", ")
                )
            }
            ConsensusResult::Rejected {
                proposal, reason, ..
            } => {
                format!("REJECTED: '{}'\nReason: {:?}\n", proposal.title, reason)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometric_mean_calculation() {
        let council = Council::new(vec![], EudaimonicTelos::default());

        // Test with equal scores
        let votes = vec![
            ProposalAnalysis {
                agent_id: "a".into(),
                proposal_id: "p".into(),
                score: 0.8,
                is_veto: false,
                rationale: "".into(),
                concerns: vec![],
                suggestions: vec![],
            },
            ProposalAnalysis {
                agent_id: "b".into(),
                proposal_id: "p".into(),
                score: 0.8,
                is_veto: false,
                rationale: "".into(),
                concerns: vec![],
                suggestions: vec![],
            },
        ];

        let mean = council.calculate_geometric_mean(&votes);
        assert!((mean - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_geometric_mean_with_low_score() {
        let council = Council::new(vec![], EudaimonicTelos::default());

        // One low score drags down the mean
        let votes = vec![
            ProposalAnalysis {
                agent_id: "a".into(),
                proposal_id: "p".into(),
                score: 0.9,
                is_veto: false,
                rationale: "".into(),
                concerns: vec![],
                suggestions: vec![],
            },
            ProposalAnalysis {
                agent_id: "b".into(),
                proposal_id: "p".into(),
                score: 0.1,
                is_veto: false,
                rationale: "".into(),
                concerns: vec![],
                suggestions: vec![],
            },
        ];

        let mean = council.calculate_geometric_mean(&votes);
        // sqrt(0.9 * 0.1) = sqrt(0.09) ≈ 0.3
        assert!(mean < 0.35);
        assert!(mean > 0.25);
    }
}
