use crate::core::ethics::{EthicalImpactAssessment, EthicsManager};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Categories of optimization goals that the agent can pursue
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(Default)]
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
    #[default]
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

impl From<PriorityLevel> for u8 {
    fn from(level: PriorityLevel) -> Self {
        match level {
            PriorityLevel::Low => 25,
            PriorityLevel::Medium => 50,
            PriorityLevel::High => 75,
            PriorityLevel::Critical => 100,
        }
    }
}

impl From<u8> for PriorityLevel {
    fn from(value: u8) -> Self {
        match value {
            0..=30 => PriorityLevel::Low,
            31..=60 => PriorityLevel::Medium,
            61..=90 => PriorityLevel::High,
            _ => PriorityLevel::Critical,
        }
    }
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

/// Resource estimates for a goal
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceEstimates {
    /// Estimated time to complete in hours
    #[serde(default)]
    pub time_hours: f64,

    /// Estimated complexity on a scale of 1-10
    #[serde(default)]
    pub complexity: u8,

    /// Estimated memory usage in MB
    #[serde(default)]
    pub memory_mb: Option<f64>,

    /// Estimated CPU usage percentage
    #[serde(default)]
    pub cpu_percent: Option<f64>,
}

/// Example code snippet for a goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSample {
    /// Description of what this code sample demonstrates
    pub description: String,

    /// The actual code
    pub code: String,

    /// The language of the code
    pub language: String,
}

/// Criteria for filtering goals
#[derive(Debug, Clone, Default)]
pub struct FilterCriteria {
    /// Filter by status
    pub status: Option<GoalStatus>,

    /// Filter by area (tag content)
    pub area: Option<String>,

    /// Filter by minimum priority
    pub min_priority: Option<u8>,

    /// Filter by specific tags that must all be present
    pub tags: Vec<String>,
}

/// Default priority for optimization goals
fn default_priority() -> u8 {
    50 // Medium priority (1-100 scale)
}

/// Detailed specification of an optimization goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationGoal {
    /// Unique identifier for the goal
    pub id: String,

    /// Title of the goal
    pub title: String,

    /// Detailed description of what should be improved
    pub description: String,

    /// Status of the goal
    pub status: GoalStatus,

    /// Creation date
    #[serde(default = "chrono::Utc::now")]
    pub created_at: DateTime<Utc>,

    /// Last update date
    #[serde(default = "chrono::Utc::now")]
    pub updated_at: DateTime<Utc>,

    /// Optional resource estimates
    #[serde(default)]
    pub resources: ResourceEstimates,

    /// Optional priority (1-100, higher is more important)
    #[serde(default = "default_priority")]
    pub priority: u8,

    /// Associated strategic objective ID
    #[serde(default)]
    pub objective_id: Option<String>,

    /// Associated milestone ID
    #[serde(default)]
    pub milestone_id: Option<String>,

    /// Related tactical goals
    #[serde(default)]
    pub related_goals: Vec<String>,

    /// Dependencies (goals that must be completed first)
    #[serde(default)]
    pub dependencies: Vec<String>,

    /// Improvement level aimed for (-5 to 5, negative is regression)
    #[serde(default)]
    pub improvement_target: i8,

    /// Ethical considerations
    #[serde(default)]
    pub ethical_considerations: Vec<String>,

    /// Optional code samples for improvement
    #[serde(default)]
    pub code_samples: Vec<CodeSample>,

    /// Implementation details (if any)
    #[serde(default)]
    pub implementation: Option<String>,

    /// Test results (if any)
    #[serde(default)]
    pub test_results: Option<String>,

    /// Optional tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,

    /// Success metrics for this goal
    #[serde(default)]
    pub success_metrics: Vec<String>,

    /// Notes about implementation approach
    #[serde(default)]
    pub implementation_notes: Option<String>,

    /// Ethical assessment results
    #[serde(default)]
    pub ethical_assessment: Option<EthicalImpactAssessment>,

    /// Category of optimization
    #[serde(default)]
    pub category: OptimizationCategory,
}

impl OptimizationGoal {
    /// Create a new optimization goal
    pub fn new(id: &str, title: &str, description: &str) -> Self {
        let now = Utc::now();
        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            status: GoalStatus::NotStarted,
            created_at: now,
            updated_at: now,
            resources: ResourceEstimates::default(),
            priority: default_priority(),
            objective_id: None,
            milestone_id: None,
            related_goals: Vec::new(),
            dependencies: Vec::new(),
            improvement_target: 0,
            ethical_considerations: Vec::new(),
            code_samples: Vec::new(),
            implementation: None,
            test_results: None,
            tags: Vec::new(),
            success_metrics: Vec::new(),
            implementation_notes: None,
            ethical_assessment: None,
            category: OptimizationCategory::General,
        }
    }

    /// Check if this goal is associated with a specific strategic objective
    pub fn is_part_of_objective(&self, objective_id: &str) -> bool {
        self.objective_id
            .as_ref()
            .is_some_and(|id| id == objective_id)
    }

    /// Get a summary of the goal
    pub fn summary(&self) -> String {
        format!(
            "Goal '{}': {} (Status: {})",
            self.id, self.title, self.status
        )
    }

    /// Get a detailed description of the goal
    pub fn details(&self) -> String {
        let mut details = format!(
            "# Goal: {}\n\n## Description\n{}\n\n## Status\n{}\n\n",
            self.title, self.description, self.status
        );

        if let Some(objective_id) = &self.objective_id {
            details.push_str(&format!("## Strategic Objective\n{}\n\n", objective_id));
        }

        if !self.dependencies.is_empty() {
            details.push_str("## Dependencies\n");
            for dep in &self.dependencies {
                details.push_str(&format!("- {}\n", dep));
            }
            details.push('\n');
        }

        if !self.ethical_considerations.is_empty() {
            details.push_str("## Ethical Considerations\n");
            for consideration in &self.ethical_considerations {
                details.push_str(&format!("- {}\n", consideration));
            }
            details.push('\n');
        }

        details
    }

    /// Evaluate if this goal matches a specific area
    pub fn matches_area(&self, area: &str) -> bool {
        // Match by tag
        self.tags.iter().any(|tag| tag.contains(area))
        // Match by title or description
        || self.title.to_lowercase().contains(&area.to_lowercase())
        || self.description.to_lowercase().contains(&area.to_lowercase())
    }

    /// Add a dependency to this goal
    pub fn add_dependency(&mut self, goal_id: &str) {
        // Only add if not already present
        if !self.dependencies.contains(&goal_id.to_string()) {
            self.dependencies.push(goal_id.to_string());
        }
    }

    /// Update the status of this goal
    pub fn update_status(&mut self, new_status: GoalStatus) {
        self.status = new_status;
        self.updated_at = chrono::Utc::now();
    }

    /// Conduct an ethical assessment of this goal
    pub fn assess_ethics(&mut self, ethics_manager: &mut EthicsManager) {
        let assessment = ethics_manager.assess_ethical_impact(
            &self.description,
            &self.implementation.clone().unwrap_or_default(),
        );

        self.ethical_assessment = Some(assessment);
        self.updated_at = chrono::Utc::now();
    }

    /// Check if this goal has been ethically assessed and approved
    pub fn is_ethically_sound(&self) -> bool {
        if let Some(assessment) = &self.ethical_assessment {
            assessment.is_approved
        } else {
            false
        }
    }

    /// Update the priority of this goal
    pub fn update_priority(&mut self, new_priority: PriorityLevel) {
        self.priority = new_priority.into();
        self.updated_at = chrono::Utc::now();
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

    /// Clear all goals
    pub fn clear_goals(&mut self) {
        self.goals.clear();
    }

    /// Get goals by status
    pub fn get_goals_by_status(&self, status: GoalStatus) -> Vec<&OptimizationGoal> {
        self.goals.iter().filter(|g| g.status == status).collect()
    }

    /// Get goals by category
    pub fn get_goals_by_category(&self, category: OptimizationCategory) -> Vec<&OptimizationGoal> {
        let category_str = category.to_string().to_lowercase();
        self.goals
            .iter()
            .filter(|g| g.category == category || g.tags.iter().any(|t| t == &category_str))
            .collect()
    }

    /// Get goals by priority
    pub fn get_goals_by_priority(&self, priority: PriorityLevel) -> Vec<&OptimizationGoal> {
        let priority_val = u8::from(priority);
        self.goals
            .iter()
            .filter(|g| g.priority == priority_val)
            .collect()
    }

    /// Get goals that affect a specific file or area
    pub fn get_goals_by_affected_area(&self, area: &str) -> Vec<&OptimizationGoal> {
        self.goals
            .iter()
            .filter(|g| g.tags.iter().any(|t| t.contains(area)))
            .collect()
    }

    /// Get the next most important goal to work on
    pub fn get_next_goal(&self) -> Option<&OptimizationGoal> {
        // Get not started goals sorted by priority
        let mut candidate_goals: Vec<&OptimizationGoal> = self
            .goals
            .iter()
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
    pub fn generate_goal(
        &self,
        analysis_result: &str,
        affected_files: &[String],
        category: OptimizationCategory,
    ) -> OptimizationGoal {
        // Generate a unique ID
        let id = format!(
            "OPT-{}-{}",
            category.to_string().chars().next().unwrap_or('X'),
            chrono::Utc::now().timestamp()
        );

        // Create a basic title from the first line of analysis
        let title = analysis_result
            .lines()
            .next()
            .unwrap_or("Optimization opportunity")
            .to_string();

        // Create a new goal with a clone of the category
        let mut goal = OptimizationGoal::new(&id, &title, analysis_result);

        // Add affected files as tags
        for file in affected_files {
            goal.tags.push(format!("file:{}", file));
        }

        // Add category as a tag
        goal.tags.push(category.to_string().to_lowercase());

        // Set priority based on category (just an example heuristic)
        goal.priority = match category {
            OptimizationCategory::Financial => u8::from(PriorityLevel::Critical), // Highest priority for financial goals
            OptimizationCategory::Security => u8::from(PriorityLevel::Critical),
            OptimizationCategory::Performance => u8::from(PriorityLevel::High),
            OptimizationCategory::ErrorHandling => u8::from(PriorityLevel::High),
            _ => u8::from(PriorityLevel::Medium),
        };

        goal
    }

    /// Update goal dependences based on affected areas
    pub fn update_goal_dependencies(&mut self) {
        // First collect all goal IDs and their file tags
        let goal_files: Vec<(String, Vec<String>)> = self
            .goals
            .iter()
            .map(|g| {
                let file_tags = g
                    .tags
                    .iter()
                    .filter(|t| t.starts_with("file:"))
                    .cloned()
                    .collect();
                (g.id.clone(), file_tags)
            })
            .collect();

        // Pre-compute dependencies for each goal
        let mut new_dependencies: HashMap<String, Vec<String>> = HashMap::new();

        for goal in &self.goals {
            // Get this goal's file tags
            let goal_file_tags: Vec<&String> = goal
                .tags
                .iter()
                .filter(|t| t.starts_with("file:"))
                .collect();

            // Find other goals that affect the same files
            let mut dependencies = Vec::new();

            for (other_id, file_tags) in &goal_files {
                // Skip self
                if &goal.id == other_id {
                    continue;
                }

                // Check if there's any overlap in affected files
                let has_overlap = file_tags.iter().any(|tag| goal_file_tags.contains(&tag));

                if has_overlap {
                    dependencies.push(other_id.clone());
                }
            }

            // Store dependencies for this goal
            if !dependencies.is_empty() {
                new_dependencies.insert(goal.id.clone(), dependencies);
            }
        }

        // Now apply the dependencies
        for goal in &mut self.goals {
            if let Some(deps) = new_dependencies.get(&goal.id) {
                for dep_id in deps {
                    goal.add_dependency(dep_id);
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

/// Filter goals by specific criteria
pub fn filter_goals(
    goals: &[OptimizationGoal],
    criteria: &FilterCriteria,
) -> Vec<OptimizationGoal> {
    let mut filtered = goals.to_vec();

    // Filter by status if specified
    if let Some(status) = &criteria.status {
        filtered.retain(|g| &g.status == status);
    }

    // Filter by area if specified
    if let Some(area) = &criteria.area {
        filtered.retain(|g| g.matches_area(area));
    }

    // Filter by priority if specified
    if let Some(min_priority) = criteria.min_priority {
        filtered.retain(|g| g.priority >= min_priority);
    }

    // Filter by tags if specified
    if !criteria.tags.is_empty() {
        filtered.retain(|g| criteria.tags.iter().all(|tag| g.tags.contains(tag)));
    }

    filtered
}

/// Assign affected areas to a goal based on analysis
pub fn assign_affected_areas(goal: &mut OptimizationGoal, affected_files: &[String]) {
    // Add the files as tags with a file: prefix
    for file in affected_files {
        let file_tag = format!("file:{}", file);
        // Only add as a tag if it's not already there
        if !goal.tags.contains(&file_tag) {
            goal.tags.push(file_tag);
        }
    }
}

/// Get a map of conflicting goals
pub fn get_conflicting_goals(goals: &[OptimizationGoal]) -> HashMap<String, Vec<String>> {
    let mut conflicts = HashMap::new();

    // Create a map of file to goals that modify it
    let mut file_goals: HashMap<String, Vec<String>> = HashMap::new();

    for goal in goals.iter().filter(|g| g.status == GoalStatus::InProgress) {
        // Use tags that look like file paths as affected areas
        let file_paths: Vec<String> = goal
            .tags
            .iter()
            .filter(|tag| tag.contains('/') || tag.contains('\\'))
            .cloned()
            .collect();

        for tag in file_paths {
            file_goals
                .entry(tag)
                .or_default()
                .push(goal.id.clone());
        }
    }

    // Find goals that overlap in affected areas
    for goal in goals.iter().filter(|g| g.status == GoalStatus::InProgress) {
        let mut conflicting = Vec::new();

        // Get file paths from tags
        let file_paths: Vec<String> = goal
            .tags
            .iter()
            .filter(|tag| tag.contains('/') || tag.contains('\\'))
            .cloned()
            .collect();

        for tag in file_paths {
            if let Some(file_goals) = file_goals.get(&tag) {
                // Add goals that affect the same file, excluding the current goal
                for other_goal in file_goals {
                    if other_goal != &goal.id && !conflicting.contains(other_goal) {
                        conflicting.push(other_goal.clone());
                    }
                }
            }
        }

        if !conflicting.is_empty() {
            conflicts.insert(goal.id.clone(), conflicting);
        }
    }

    conflicts
}
