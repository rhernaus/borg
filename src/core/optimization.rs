use serde::{Deserialize, Serialize};
use std::fmt;
use crate::core::ethics::{EthicsManager, EthicalImpactAssessment, RiskLevel};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Categories of optimization goals that the agent can pursue
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OptimizationCategory {
    /// Improve code performance (speed, memory usage, etc.)
    Performance,

    /// Improve code readability and maintainability
    Readability,

    /// Improve test coverage and quality
    TestCoverage,

    /// Improve security posture
    Security,

    /// Reduce complexity
    Complexity,

    /// Improve error handling
    ErrorHandling,

    /// Improve compatibility
    Compatibility,

    /// Financial optimizations and improvements
    Financial,

    /// General improvements (not fitting other categories)
    General,
}

impl fmt::Display for OptimizationCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptimizationCategory::Performance => write!(f, "Performance"),
            OptimizationCategory::Readability => write!(f, "Readability"),
            OptimizationCategory::TestCoverage => write!(f, "Test Coverage"),
            OptimizationCategory::Security => write!(f, "Security"),
            OptimizationCategory::Complexity => write!(f, "Complexity"),
            OptimizationCategory::ErrorHandling => write!(f, "Error Handling"),
            OptimizationCategory::Compatibility => write!(f, "Compatibility"),
            OptimizationCategory::Financial => write!(f, "Financial"),
            OptimizationCategory::General => write!(f, "General"),
        }
    }
}

/// Priority level for optimization goals
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PriorityLevel {
    /// Low priority - can be addressed when time allows
    Low = 0,

    /// Medium priority - should be addressed in the near future
    Medium = 1,

    /// High priority - should be addressed soon
    High = 2,

    /// Critical priority - should be addressed immediately
    Critical = 3,
}

impl fmt::Display for PriorityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PriorityLevel::Low => write!(f, "Low"),
            PriorityLevel::Medium => write!(f, "Medium"),
            PriorityLevel::High => write!(f, "High"),
            PriorityLevel::Critical => write!(f, "Critical"),
        }
    }
}

/// Status of an optimization goal
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GoalStatus {
    /// Goal has been identified but not started
    NotStarted,

    /// Goal is currently being worked on
    InProgress,

    /// Goal has been successfully completed
    Completed,

    /// Goal was attempted but could not be completed
    Failed,

    /// Goal was determined to be not feasible or not beneficial
    Abandoned,
}

impl fmt::Display for GoalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GoalStatus::NotStarted => write!(f, "Not Started"),
            GoalStatus::InProgress => write!(f, "In Progress"),
            GoalStatus::Completed => write!(f, "Completed"),
            GoalStatus::Failed => write!(f, "Failed"),
            GoalStatus::Abandoned => write!(f, "Abandoned"),
        }
    }
}

/// Detailed specification of an optimization goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationGoal {
    /// Unique identifier for the goal
    pub id: String,

    /// Title/summary of the goal
    pub title: String,

    /// Detailed description of the goal
    pub description: String,

    /// Category of the optimization
    pub category: OptimizationCategory,

    /// Priority level
    pub priority: PriorityLevel,

    /// Current status
    pub status: GoalStatus,

    /// Specific files or modules affected
    pub affected_areas: Vec<String>,

    /// Metrics to measure success (e.g., "Reduce execution time by 20%")
    pub success_metrics: Vec<String>,

    /// Notes on implementation approach
    pub implementation_notes: Option<String>,

    /// Timestamp when the goal was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Timestamp when the goal was last updated
    pub updated_at: chrono::DateTime<chrono::Utc>,

    /// Ethical impact assessment, if performed
    pub ethical_assessment: Option<EthicalImpactAssessment>,

    /// Dependencies on other goals (by ID)
    pub dependencies: Vec<String>,
}

impl OptimizationGoal {
    /// Create a new optimization goal with default values
    pub fn new(id: &str, title: &str, description: &str, category: OptimizationCategory) -> Self {
        let now = chrono::Utc::now();

        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            category,
            priority: PriorityLevel::Medium,
            status: GoalStatus::NotStarted,
            affected_areas: Vec::new(),
            success_metrics: Vec::new(),
            implementation_notes: None,
            created_at: now,
            updated_at: now,
            ethical_assessment: None,
            dependencies: Vec::new(),
        }
    }

    /// Change the status of the goal
    pub fn update_status(&mut self, new_status: GoalStatus) {
        self.status = new_status;
        self.updated_at = chrono::Utc::now();
    }

    /// Update the priority of the goal
    pub fn update_priority(&mut self, new_priority: PriorityLevel) {
        self.priority = new_priority;
        self.updated_at = chrono::Utc::now();
    }

    /// Add a dependency on another goal
    pub fn add_dependency(&mut self, goal_id: &str) {
        if !self.dependencies.contains(&goal_id.to_string()) {
            self.dependencies.push(goal_id.to_string());
            self.updated_at = chrono::Utc::now();
        }
    }

    /// Remove a dependency
    pub fn remove_dependency(&mut self, goal_id: &str) {
        self.dependencies.retain(|id| id != goal_id);
        self.updated_at = chrono::Utc::now();
    }

    /// Perform an ethical assessment of this goal
    pub fn assess_ethics(&mut self, ethics_manager: &mut EthicsManager) {
        // Create an assessment based on the goal
        let assessment = ethics_manager.assess_ethical_impact(
            &self.description,
            &self.implementation_notes.clone().unwrap_or_default(),
            &self.affected_areas,
        );

        self.ethical_assessment = Some(assessment);
        self.updated_at = chrono::Utc::now();
    }

    /// Check if the goal meets ethical standards
    pub fn is_ethically_sound(&self) -> bool {
        match &self.ethical_assessment {
            Some(assessment) => {
                assessment.is_approved && assessment.risk_level < RiskLevel::High
            }
            None => false // Cannot determine without an assessment
        }
    }
}

/// Manages the optimization goals for the agent
pub struct OptimizationManager {
    /// All current optimization goals
    goals: Vec<OptimizationGoal>,

    /// Reference to the ethics manager
    ethics_manager: Arc<Mutex<EthicsManager>>,
}

impl OptimizationManager {
    /// Create a new optimization manager
    pub fn new(ethics_manager: Arc<Mutex<EthicsManager>>) -> Self {
        Self {
            goals: Vec::new(),
            ethics_manager,
        }
    }

    /// Add a new optimization goal
    pub fn add_goal(&mut self, goal: OptimizationGoal) {
        self.goals.push(goal);
    }

    /// Get a goal by its ID
    pub fn get_goal(&self, id: &str) -> Option<&OptimizationGoal> {
        self.goals.iter().find(|g| g.id == id)
    }

    /// Get a mutable reference to a goal by its ID
    pub fn get_goal_mut(&mut self, id: &str) -> Option<&mut OptimizationGoal> {
        self.goals.iter_mut().find(|g| g.id == id)
    }

    /// Remove a goal by its ID
    pub fn remove_goal(&mut self, id: &str) {
        self.goals.retain(|g| g.id != id);
    }

    /// Get all goals
    pub fn get_all_goals(&self) -> &[OptimizationGoal] {
        &self.goals
    }

    /// Get goals by status
    pub fn get_goals_by_status(&self, status: GoalStatus) -> Vec<&OptimizationGoal> {
        self.goals.iter().filter(|g| g.status == status).collect()
    }

    /// Get goals by category
    pub fn get_goals_by_category(&self, category: OptimizationCategory) -> Vec<&OptimizationGoal> {
        self.goals.iter().filter(|g| g.category == category).collect()
    }

    /// Get goals by priority
    pub fn get_goals_by_priority(&self, priority: PriorityLevel) -> Vec<&OptimizationGoal> {
        self.goals.iter().filter(|g| g.priority == priority).collect()
    }

    /// Get goals that affect a specific file or area
    pub fn get_goals_by_affected_area(&self, area: &str) -> Vec<&OptimizationGoal> {
        self.goals.iter()
            .filter(|g| g.affected_areas.iter().any(|a| a.contains(area)))
            .collect()
    }

    /// Get the next most important goal to work on
    pub fn get_next_goal(&self) -> Option<&OptimizationGoal> {
        // Get not started goals sorted by priority
        let mut candidate_goals: Vec<&OptimizationGoal> = self.goals.iter()
            .filter(|g| g.status == GoalStatus::NotStarted)
            .collect();

        // Sort by priority (highest first)
        candidate_goals.sort_by(|a, b| b.priority.cmp(&a.priority));

        // Get the highest priority goal
        candidate_goals.first().copied()
    }

    /// Assess the ethics of all goals
    pub async fn assess_all_goals_ethics(&mut self) {
        let mut ethics_manager = self.ethics_manager.lock().await;
        for goal in &mut self.goals {
            goal.assess_ethics(&mut ethics_manager);
        }
    }

    /// Generate a new optimization goal based on analysis
    pub fn generate_goal(&self,
                        analysis_result: &str,
                        affected_files: &[String],
                        category: OptimizationCategory) -> OptimizationGoal {
        // Generate a unique ID
        let id = format!("OPT-{}-{}",
                         category.to_string().chars().next().unwrap_or('X'),
                         chrono::Utc::now().timestamp());

        // Create a basic title from the first line of analysis
        let title = analysis_result.lines()
            .next()
            .unwrap_or("Optimization opportunity")
            .to_string();

        // Create a new goal with a clone of the category
        let mut goal = OptimizationGoal::new(&id, &title, analysis_result, category.clone());

        // Set affected areas
        goal.affected_areas = affected_files.to_vec();

        // Set priority based on category (just an example heuristic)
        goal.priority = match category {
            OptimizationCategory::Financial => PriorityLevel::Critical, // Highest priority for financial goals
            OptimizationCategory::Security => PriorityLevel::Critical,
            OptimizationCategory::Performance => PriorityLevel::High,
            OptimizationCategory::ErrorHandling => PriorityLevel::High,
            _ => PriorityLevel::Medium,
        };

        goal
    }

    /// Update goal dependences based on affected areas
    pub fn update_goal_dependencies(&mut self) {
        // First collect all goal IDs and their affected areas
        let goal_areas: Vec<(String, Vec<String>)> = self.goals
            .iter()
            .map(|g| (g.id.clone(), g.affected_areas.clone()))
            .collect();

        // Then update dependencies for each goal
        for goal in &mut self.goals {
            // Find other goals that affect the same areas
            for (other_id, areas) in &goal_areas {
                // Skip self
                if &goal.id == other_id {
                    continue;
                }

                // Check if there's any overlap in affected areas
                let has_overlap = areas.iter()
                    .any(|area| goal.affected_areas.contains(area));

                if has_overlap {
                    goal.add_dependency(other_id);
                }
            }
        }
    }

    /// Get a reference to the ethics manager
    pub fn ethics_manager(&self) -> &Arc<Mutex<EthicsManager>> {
        &self.ethics_manager
    }

    /// Get a mutable reference to the ethics manager
    pub fn ethics_manager_mut(&mut self) -> &Arc<Mutex<EthicsManager>> {
        &self.ethics_manager
    }
}