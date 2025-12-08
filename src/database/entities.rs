use crate::core::optimization::OptimizationGoal;
use crate::database::models::Entity;
use std::marker::Unpin;

/// Implementation of Entity trait for OptimizationGoal
impl Entity for OptimizationGoal {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }
}

// Implement Unpin for all entity types
impl Unpin for OptimizationGoal {}
