use crate::core::optimization::OptimizationGoal;
use crate::core::planning::{Milestone, StrategicObjective, StrategicPlan};
use crate::database::models::Entity;
use serde::{Deserialize, Serialize};
use std::marker::Unpin;

/// Implementation of Entity trait for StrategicObjective
impl Entity for StrategicObjective {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }
}

/// Implementation of Entity trait for Milestone
impl Entity for Milestone {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }
}

/// Implementation of Entity trait for OptimizationGoal
impl Entity for OptimizationGoal {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }
}

/// A wrapper for StrategicPlan to implement Entity trait
/// This is needed because StrategicPlan doesn't have an ID field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicPlanEntity {
    /// Unique ID for the plan (using "current" as default)
    pub id: String,

    /// The actual strategic plan
    pub plan: StrategicPlan,
}

impl StrategicPlanEntity {
    /// Create a new strategic plan entity with the given plan
    pub fn new(plan: StrategicPlan) -> Self {
        Self {
            id: "current".to_string(),
            plan,
        }
    }

    /// Get the current plan
    pub fn plan(&self) -> &StrategicPlan {
        &self.plan
    }
}

impl Entity for StrategicPlanEntity {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }
}

// Implement Unpin for all entity types
impl Unpin for StrategicObjective {}
impl Unpin for Milestone {}
impl Unpin for OptimizationGoal {}
impl Unpin for StrategicPlanEntity {}
