use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::{Result, anyhow};
use log::{info, warn, error};
use chrono::{DateTime, Utc};
use std::path::Path;
use std::fs;

use crate::core::optimization::{OptimizationManager, OptimizationGoal, OptimizationCategory, PriorityLevel, GoalStatus};
use crate::core::ethics::EthicsManager;

/// Status of a planning milestone
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MilestoneStatus {
    /// Milestone has been defined but work hasn't started
    Planned,

    /// Work on achieving this milestone has started
    InProgress,

    /// Milestone has been achieved
    Achieved,

    /// Milestone was abandoned
    Abandoned,

    /// Milestone was redefined or replaced
    Superseded,
}

impl fmt::Display for MilestoneStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MilestoneStatus::Planned => write!(f, "Planned"),
            MilestoneStatus::InProgress => write!(f, "In Progress"),
            MilestoneStatus::Achieved => write!(f, "Achieved"),
            MilestoneStatus::Abandoned => write!(f, "Abandoned"),
            MilestoneStatus::Superseded => write!(f, "Superseded"),
        }
    }
}

/// Strategic objective defined by the creator for long-term direction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicObjective {
    /// Unique identifier for the objective
    pub id: String,

    /// Title of the strategic objective
    pub title: String,

    /// Detailed description of the objective
    pub description: String,

    /// Expected timeframe for achievement (in months)
    pub timeframe: u32,

    /// Key result areas that would indicate success
    pub key_results: Vec<String>,

    /// Constraints or boundaries to operate within
    pub constraints: Vec<String>,

    /// When this objective was created
    pub created_at: DateTime<Utc>,

    /// Who created this objective (must be a creator role)
    pub created_by: String,

    /// Current progress (0-100)
    pub progress: u8,
}

impl StrategicObjective {
    /// Create a new strategic objective
    pub fn new(
        id: &str,
        title: &str,
        description: &str,
        timeframe: u32,
        creator: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            timeframe,
            key_results: Vec::new(),
            constraints: Vec::new(),
            created_at: Utc::now(),
            created_by: creator.to_string(),
            progress: 0,
        }
    }

    /// Add key results to this objective
    pub fn with_key_results(mut self, key_results: Vec<String>) -> Self {
        self.key_results = key_results;
        self
    }

    /// Add constraints to this objective
    pub fn with_constraints(mut self, constraints: Vec<String>) -> Self {
        self.constraints = constraints;
        self
    }
}

/// A milestone represents a significant achievement on the path to a strategic objective
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    /// Unique identifier for the milestone
    pub id: String,

    /// Title of the milestone
    pub title: String,

    /// Detailed description
    pub description: String,

    /// The strategic objective this milestone contributes to
    pub parent_objective_id: String,

    /// Expected completion date
    pub target_date: DateTime<Utc>,

    /// Current status
    pub status: MilestoneStatus,

    /// Success criteria for this milestone
    pub success_criteria: Vec<String>,

    /// Dependencies on other milestones (IDs)
    pub dependencies: Vec<String>,

    /// When this milestone was created
    pub created_at: DateTime<Utc>,

    /// When this milestone was last updated
    pub updated_at: DateTime<Utc>,

    /// Progress toward completion (0-100)
    pub progress: u8,
}

impl Milestone {
    /// Create a new milestone
    pub fn new(
        id: &str,
        title: &str,
        description: &str,
        parent_objective_id: &str,
        target_date: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            parent_objective_id: parent_objective_id.to_string(),
            target_date,
            status: MilestoneStatus::Planned,
            success_criteria: Vec::new(),
            dependencies: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            progress: 0,
        }
    }

    /// Add success criteria to this milestone
    pub fn with_success_criteria(mut self, criteria: Vec<String>) -> Self {
        self.success_criteria = criteria;
        self
    }

    /// Add dependencies to this milestone
    pub fn with_dependencies(mut self, dependencies: Vec<String>) -> Self {
        self.dependencies = dependencies;
        self
    }

    /// Check if this milestone is blocked by dependencies
    pub fn is_blocked(&self, milestones: &[Milestone]) -> bool {
        if self.dependencies.is_empty() {
            return false;
        }

        // Check if any dependencies are not achieved
        self.dependencies.iter().any(|dep_id| {
            milestones
                .iter()
                .find(|m| m.id == *dep_id)
                .map_or(true, |m| m.status != MilestoneStatus::Achieved)
        })
    }

    /// Update the milestone's progress based on related goals
    pub fn update_progress(&mut self, related_goals: &[&OptimizationGoal]) {
        if related_goals.is_empty() {
            return;
        }

        // Calculate progress based on completed goals
        let total_goals = related_goals.len();
        let completed_goals = related_goals
            .iter()
            .filter(|g| g.status == GoalStatus::Completed)
            .count();

        self.progress = if total_goals > 0 {
            ((completed_goals as f32 / total_goals as f32) * 100.0) as u8
        } else {
            0
        };

        // Update status based on progress
        if self.progress >= 100 {
            self.status = MilestoneStatus::Achieved;
        } else if self.progress > 0 {
            self.status = MilestoneStatus::InProgress;
        }

        self.updated_at = Utc::now();
    }
}

/// A plan represents a complete strategic plan with objectives, milestones, and tactical goals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategicPlan {
    /// Strategic objectives
    pub objectives: Vec<StrategicObjective>,

    /// Milestones for achieving objectives
    pub milestones: Vec<Milestone>,

    /// When this plan was created
    pub created_at: DateTime<Utc>,

    /// When this plan was last updated
    pub updated_at: DateTime<Utc>,

    /// When the last planning cycle was executed
    pub last_planning_cycle: Option<DateTime<Utc>>,
}

impl StrategicPlan {
    /// Create a new empty strategic plan
    pub fn new() -> Self {
        Self {
            objectives: Vec::new(),
            milestones: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_planning_cycle: None,
        }
    }

    /// Add a strategic objective to the plan
    pub fn add_objective(&mut self, objective: StrategicObjective) {
        self.objectives.push(objective);
        self.updated_at = Utc::now();
    }

    /// Add a milestone to the plan
    pub fn add_milestone(&mut self, milestone: Milestone) {
        self.milestones.push(milestone);
        self.updated_at = Utc::now();
    }

    /// Get objectives by creator
    pub fn get_objectives_by_creator(&self, creator: &str) -> Vec<&StrategicObjective> {
        self.objectives
            .iter()
            .filter(|o| o.created_by == creator)
            .collect()
    }

    /// Get milestones for a specific objective
    pub fn get_milestones_for_objective(&self, objective_id: &str) -> Vec<&Milestone> {
        self.milestones
            .iter()
            .filter(|m| m.parent_objective_id == objective_id)
            .collect()
    }

    /// Get milestones by status
    pub fn get_milestones_by_status(&self, status: MilestoneStatus) -> Vec<&Milestone> {
        self.milestones
            .iter()
            .filter(|m| m.status == status)
            .collect()
    }

    /// Get milestones that are ready to work on (not blocked by dependencies)
    pub fn get_ready_milestones(&self) -> Vec<&Milestone> {
        self.milestones
            .iter()
            .filter(|m|
                m.status == MilestoneStatus::Planned &&
                !m.is_blocked(&self.milestones)
            )
            .collect()
    }

    /// Update the progress of objectives based on milestone progress
    pub fn update_objectives_progress(&mut self) {
        for objective in &mut self.objectives {
            // Clone the milestones IDs to avoid borrowing self immutably and mutably at the same time
            let milestone_ids: Vec<String> = self.milestones.iter()
                .filter(|m| m.parent_objective_id == objective.id)
                .map(|m| m.id.clone())
                .collect();

            if milestone_ids.is_empty() {
                continue;
            }

            let total_milestones = milestone_ids.len();
            let achieved_milestones = self.milestones.iter()
                .filter(|m| milestone_ids.contains(&m.id) && m.status == MilestoneStatus::Achieved)
                .count();

            objective.progress = if total_milestones > 0 {
                ((achieved_milestones as f32 / total_milestones as f32) * 100.0) as u8
            } else {
                0
            };
        }

        self.updated_at = Utc::now();
    }
}

/// Manages strategic planning for the agent
pub struct StrategicPlanningManager {
    /// The current strategic plan
    plan: StrategicPlan,

    /// Reference to the optimization manager
    optimization_manager: Arc<Mutex<OptimizationManager>>,

    /// Reference to the ethics manager
    ethics_manager: Arc<Mutex<EthicsManager>>,

    /// Directory to store planning data
    data_dir: String,
}

impl StrategicPlanningManager {
    /// Create a new strategic planning manager
    pub fn new(
        optimization_manager: Arc<Mutex<OptimizationManager>>,
        ethics_manager: Arc<Mutex<EthicsManager>>,
        data_dir: &str,
    ) -> Self {
        Self {
            plan: StrategicPlan::new(),
            optimization_manager,
            ethics_manager,
            data_dir: data_dir.to_string(),
        }
    }

    /// Load the strategic plan from disk
    pub async fn load_from_disk(&mut self) -> Result<()> {
        let plan_path = Path::new(&self.data_dir).join("strategic_plan.json");

        if !plan_path.exists() {
            info!("No strategic plan found on disk, initializing empty plan");
            return Ok(());
        }

        let plan_json = fs::read_to_string(plan_path)?;
        self.plan = serde_json::from_str(&plan_json)?;

        info!("Loaded strategic plan with {} objectives and {} milestones",
            self.plan.objectives.len(),
            self.plan.milestones.len()
        );

        Ok(())
    }

    /// Save the strategic plan to disk
    pub async fn save_to_disk(&self) -> Result<()> {
        let data_dir = Path::new(&self.data_dir);

        if !data_dir.exists() {
            fs::create_dir_all(data_dir)?;
        }

        let plan_path = data_dir.join("strategic_plan.json");
        let plan_json = serde_json::to_string_pretty(&self.plan)?;

        // Write atomically to avoid corruption
        let temp_path = plan_path.with_extension("tmp");
        fs::write(&temp_path, plan_json)?;
        fs::rename(temp_path, plan_path)?;

        info!("Saved strategic plan to disk");

        Ok(())
    }

    /// Add a strategic objective
    pub fn add_objective(&mut self, objective: StrategicObjective) {
        self.plan.add_objective(objective);
    }

    /// Add a milestone
    pub fn add_milestone(&mut self, milestone: Milestone) {
        self.plan.add_milestone(milestone);
    }

    /// Get objectives by creator
    pub fn get_objectives_by_creator(&self, creator: &str) -> Vec<&StrategicObjective> {
        self.plan.get_objectives_by_creator(creator)
    }

    /// Get all objectives
    pub fn get_all_objectives(&self) -> &[StrategicObjective] {
        &self.plan.objectives
    }

    /// Get all milestones
    pub fn get_all_milestones(&self) -> &[Milestone] {
        &self.plan.milestones
    }

    /// Get milestones for a specific objective
    pub fn get_milestones_for_objective(&self, objective_id: &str) -> Vec<&Milestone> {
        self.plan.get_milestones_for_objective(objective_id)
    }

    /// Get active milestones (planned or in progress)
    pub fn get_active_milestones(&self) -> Vec<&Milestone> {
        self.plan.milestones
            .iter()
            .filter(|m| m.status == MilestoneStatus::Planned || m.status == MilestoneStatus::InProgress)
            .collect()
    }

    /// Check if a planning cycle is due
    pub fn is_planning_cycle_due(&self) -> bool {
        match self.plan.last_planning_cycle {
            None => true, // Never ran a planning cycle, so one is due
            Some(last_cycle) => {
                // Run planning cycle weekly
                let week_in_seconds = 7 * 24 * 60 * 60;
                let now = Utc::now();
                let duration = now.signed_duration_since(last_cycle);

                duration.num_seconds() >= week_in_seconds
            }
        }
    }

    /// Generate milestones for an objective using LLM
    pub async fn generate_milestones_for_objective(&self, objective: &StrategicObjective) -> Result<Vec<Milestone>> {
        // In a real implementation, this would use the LLM to generate milestones
        // For this implementation, we'll create simulated milestones

        let milestone_count = 3; // Typically 3-5 milestones per objective
        let mut milestones = Vec::new();

        let timeframe_months = objective.timeframe as i64;
        let now = Utc::now();

        for i in 1..=milestone_count {
            let percentage = (i as f32) / (milestone_count as f32);
            let months_offset = (timeframe_months as f32 * percentage) as i64;

            // Create milestone with target date proportional to its position
            let target_date = now + chrono::Duration::days(months_offset * 30);

            let milestone_id = format!("{}-m{}", objective.id, i);
            let milestone_title = format!("Milestone {} for {}", i, objective.title);
            let milestone_desc = format!("Achieve {}% of the objective: {}",
                (percentage * 100.0) as u8,
                objective.description
            );

            let mut milestone = Milestone::new(
                &milestone_id,
                &milestone_title,
                &milestone_desc,
                &objective.id,
                target_date,
            );

            // Add success criteria based on key results
            let success_criteria = objective.key_results
                .iter()
                .map(|kr| format!("Progress toward: {}", kr))
                .collect();

            milestone.success_criteria = success_criteria;

            // Add dependency on previous milestone
            if i > 1 {
                let prev_milestone_id = format!("{}-m{}", objective.id, i-1);
                milestone.dependencies.push(prev_milestone_id);
            }

            milestones.push(milestone);
        }

        Ok(milestones)
    }

    /// Generate tactical goals from a milestone using LLM
    pub async fn generate_goals_for_milestone(&self, milestone: &Milestone) -> Result<Vec<OptimizationGoal>> {
        // In a real implementation, this would use the LLM to generate tactical goals
        // For this implementation, we'll create simulated goals

        let goal_count = 2; // Typically 2-4 goals per milestone
        let mut goals = Vec::new();

        for i in 1..=goal_count {
            let goal_id = format!("{}-g{}", milestone.id, i);
            let goal_title = format!("Goal {} for {}", i, milestone.title);
            let goal_desc = format!("Implement functionality to support: {}", milestone.description);

            // Select a category based on milestone content
            let category = if milestone.title.contains("performance") {
                OptimizationCategory::Performance
            } else if milestone.title.contains("security") {
                OptimizationCategory::Security
            } else if milestone.title.contains("testing") {
                OptimizationCategory::TestCoverage
            } else {
                OptimizationCategory::General
            };

            let mut goal = OptimizationGoal::new(
                &goal_id,
                &goal_title,
                &goal_desc
            );

            goal.category = category;

            // Add success metrics based on milestone criteria
            let success_metrics = milestone.success_criteria
                .iter()
                .map(|sc| format!("Contribute to: {}", sc))
                .collect();

            goal.success_metrics = success_metrics;

            // Set priority based on urgency
            let urgency = if i == 1 { "critical" } else if i == 2 { "high" } else if i == 3 { "medium" } else { "low" };
            let priority = match urgency {
                "critical" => u8::from(PriorityLevel::Critical),
                "high" => u8::from(PriorityLevel::High),
                "medium" => u8::from(PriorityLevel::Medium),
                _ => u8::from(PriorityLevel::Low)
            };

            goal.priority = priority;

            goals.push(goal);
        }

        Ok(goals)
    }

    /// Update milestone status based on related goal completion
    pub async fn update_milestone_status(&mut self) -> Result<()> {
        let optimization_manager = self.optimization_manager.lock().await;
        let all_goals = optimization_manager.get_all_goals();

        // For each milestone, find related goals and update progress
        for milestone in &mut self.plan.milestones {
            let related_goals: Vec<&OptimizationGoal> = all_goals
                .iter()
                .filter(|g| {
                    g.id.starts_with(&milestone.id) ||
                    g.description.contains(&milestone.title)
                })
                .collect();

            milestone.update_progress(&related_goals);
        }

        // Update objective progress based on milestone progress
        self.plan.update_objectives_progress();

        Ok(())
    }

    /// Review progress on existing goals and milestones
    pub async fn review_progress(&mut self) -> Result<()> {
        // Update milestone status based on goal completion
        self.update_milestone_status().await?;

        // Check for milestones past their target date
        let now = Utc::now();

        for milestone in &mut self.plan.milestones {
            if milestone.status != MilestoneStatus::Achieved &&
               milestone.status != MilestoneStatus::Abandoned &&
               milestone.status != MilestoneStatus::Superseded {

                // If milestone is past due
                if milestone.target_date < now {
                    if milestone.progress > 80 {
                        // Almost complete, extend deadline
                        milestone.target_date = now + chrono::Duration::days(30);
                        info!("Extended deadline for milestone {} as it's almost complete", milestone.id);
                    } else if milestone.progress < 20 {
                        // Barely started, consider abandoning
                        milestone.status = MilestoneStatus::Abandoned;
                        info!("Abandoned milestone {} as it's past due with little progress", milestone.id);
                    } else {
                        // In progress but behind schedule, adjust target
                        milestone.target_date = now + chrono::Duration::days(60);
                        info!("Adjusted target date for milestone {} as it's behind schedule", milestone.id);
                    }
                }
            }
        }

        Ok(())
    }

    /// Prioritize tactical goals based on milestone dependencies and dates
    fn prioritize_tactical_goals(&self, goals: &mut [OptimizationGoal]) {
        // Extract milestone information first to avoid borrowing issues
        let milestone_info: Vec<(String, Option<DateTime<Utc>>, Vec<String>)> = goals.iter()
            .map(|goal| {
                let milestone_id = goal.id.split('-').next().unwrap_or("").to_string();
                let milestone = self.plan.milestones.iter().find(|m| m.id == milestone_id);

                match milestone {
                    Some(m) => (milestone_id, Some(m.target_date), m.dependencies.clone()),
                    None => (milestone_id, None, Vec::new()),
                }
            })
            .collect();

        // Create a vector of indices for sorting
        let mut indices: Vec<usize> = (0..goals.len()).collect();

        // Sort the indices based on our criteria
        indices.sort_by(|&a_idx, &b_idx| {
            let a_info = &milestone_info[a_idx];
            let b_info = &milestone_info[b_idx];

            // If one milestone depends on the other, prioritize the dependency
            if b_info.2.contains(&a_info.0) {
                return std::cmp::Ordering::Less;
            }
            if a_info.2.contains(&b_info.0) {
                return std::cmp::Ordering::Greater;
            }

            // Otherwise, sort by target date
            match (a_info.1, b_info.1) {
                (Some(a_date), Some(b_date)) => a_date.cmp(&b_date),
                _ => goals[b_idx].priority.cmp(&goals[a_idx].priority) // Fallback to priority
            }
        });

        // Reorder the goals based on the sorted indices
        // This is complex and would require cloning, so instead we'll just
        // assign priorities based on their position in the sorted indices

        let len = goals.len();
        for (pos, &idx) in indices.iter().enumerate() {
            goals[idx].priority = if pos < len / 4 {
                u8::from(PriorityLevel::Critical)
            } else if pos < len / 2 {
                u8::from(PriorityLevel::High)
            } else if pos < (len * 3) / 4 {
                u8::from(PriorityLevel::Medium)
            } else {
                u8::from(PriorityLevel::Low)
            };
        }
    }

    /// Establish dependencies between tactical goals based on milestone dependencies
    fn establish_goal_dependencies(&self, goals: &mut Vec<OptimizationGoal>) {
        // First, collect all the milestone dependencies and goal IDs
        let milestone_deps: Vec<(String, Vec<String>)> = goals.iter()
            .map(|goal| {
                let milestone_id = goal.id.split('-').next().unwrap_or("").to_string();
                let deps = self.plan.milestones.iter()
                    .find(|m| m.id == milestone_id)
                    .map(|m| m.dependencies.clone())
                    .unwrap_or_default();
                (milestone_id, deps)
            })
            .collect();

        // Clear existing dependencies
        for goal in goals.iter_mut() {
            goal.dependencies.clear();
        }

        // For each goal, establish dependencies based on milestone dependencies
        for i in 0..goals.len() {
            let milestone_id = goals[i].id.split('-').next().unwrap_or("");

            // Find the milestone dependencies for this goal
            let deps = milestone_deps.iter()
                .find(|(id, _)| id == milestone_id)
                .map(|(_, deps)| deps.clone())
                .unwrap_or_default();

            // For each milestone dependency
            for dep_milestone_id in deps {
                // Find goals related to dependency milestone
                for j in 0..goals.len() {
                    if i != j && goals[j].id.starts_with(&dep_milestone_id) {
                        // Clone the ID to avoid borrowing issues
                        let dep_id = goals[j].id.clone();
                        goals[i].dependencies.push(dep_id);
                    }
                }
            }
        }
    }

    /// Generate tactical goals from active milestones
    pub async fn generate_tactical_goals(&mut self) -> Result<Vec<OptimizationGoal>> {
        let mut tactical_goals = Vec::new();

        // For each active milestone that doesn't have complete tactical goal coverage
        for milestone in self.get_active_milestones() {
            // Generate goals for this milestone
            let mut milestone_goals = self.generate_goals_for_milestone(milestone).await?;
            tactical_goals.append(&mut milestone_goals);
        }

        // Assign priorities based on milestone dependencies and dates
        self.prioritize_tactical_goals(&mut tactical_goals);

        // Update dependencies between goals
        self.establish_goal_dependencies(&mut tactical_goals);

        Ok(tactical_goals)
    }

    /// Run a complete planning cycle
    pub async fn run_planning_cycle(&mut self) -> Result<()> {
        info!("Starting planning cycle");

        // 1. Review progress on existing goals and milestones
        self.review_progress().await?;

        // 2. Update milestone status based on completed goals
        self.update_milestone_status().await?;

        // 3. Generate milestones for any objectives without them
        // Clone the objectives first to avoid borrow checker issues
        let objectives_to_process: Vec<StrategicObjective> = self.plan.objectives.iter().cloned().collect();

        for objective in &objectives_to_process {
            let existing_milestones = self.plan.get_milestones_for_objective(&objective.id);

            if existing_milestones.is_empty() {
                // Generate milestones for this objective
                let milestones = self.generate_milestones_for_objective(objective).await?;

                for milestone in milestones {
                    self.plan.add_milestone(milestone);
                }

                info!("Generated milestones for objective: {}", objective.id);
            }
        }

        // 4. Generate new tactical goals for the next period
        let new_goals = self.generate_tactical_goals().await?;

        // 5. Add goals to the optimization manager
        let mut opt_manager = self.optimization_manager.lock().await;

        // Only add goals that don't already exist
        let existing_goal_ids: Vec<String> = opt_manager.get_all_goals()
            .iter()
            .map(|g| g.id.clone())
            .collect();

        for goal in new_goals {
            if !existing_goal_ids.contains(&goal.id) {
                opt_manager.add_goal(goal.clone());
                info!("Added new tactical goal: {}", goal.id);
            }
        }

        // 6. Record when this planning cycle was executed
        self.plan.last_planning_cycle = Some(Utc::now());

        // 7. Save the updated plan to disk
        drop(opt_manager); // Release the lock before the async call
        self.save_to_disk().await?;

        info!("Completed planning cycle");

        Ok(())
    }

    /// Generate a visualization of the planning hierarchy
    pub fn generate_planning_visualization(&self) -> Result<String> {
        let mut output = String::new();

        output.push_str("# Strategic Planning Hierarchy\n\n");

        // Add objectives
        for objective in &self.plan.objectives {
            output.push_str(&format!("## Objective: {} ({}%)\n", objective.title, objective.progress));
            output.push_str(&format!("   {}\n\n", objective.description));

            // Add milestones for this objective
            let milestones = self.plan.get_milestones_for_objective(&objective.id);
            for milestone in milestones {
                output.push_str(&format!("### Milestone: {} ({}%, {})\n",
                    milestone.title,
                    milestone.progress,
                    milestone.status
                ));
                output.push_str(&format!("    Target: {}\n", milestone.target_date.format("%Y-%m-%d")));
                output.push_str(&format!("    {}\n\n", milestone.description));

                // In a real implementation, we would add tactical goals here
            }

            output.push_str("\n");
        }

        Ok(output)
    }

    /// Generate a progress report
    pub async fn generate_progress_report(&self) -> Result<String> {
        let mut output = String::new();

        output.push_str("# Strategic Planning Progress Report\n\n");
        output.push_str(&format!("Generated: {}\n\n", Utc::now().format("%Y-%m-%d %H:%M:%S")));

        // Overall progress
        output.push_str("## Overall Progress\n\n");

        let total_objectives = self.plan.objectives.len();
        let completed_objectives = self.plan.objectives.iter()
            .filter(|o| o.progress >= 100)
            .count();

        let total_milestones = self.plan.milestones.len();
        let completed_milestones = self.plan.milestones.iter()
            .filter(|m| m.status == MilestoneStatus::Achieved)
            .count();

        output.push_str(&format!("- Objectives: {}/{} completed ({}%)\n",
            completed_objectives,
            total_objectives,
            if total_objectives > 0 { completed_objectives * 100 / total_objectives } else { 0 }
        ));

        output.push_str(&format!("- Milestones: {}/{} achieved ({}%)\n\n",
            completed_milestones,
            total_milestones,
            if total_milestones > 0 { completed_milestones * 100 / total_milestones } else { 0 }
        ));

        // Objective status
        output.push_str("## Objective Status\n\n");

        for objective in &self.plan.objectives {
            output.push_str(&format!("### {} ({}%)\n", objective.title, objective.progress));

            // Milestones progress for this objective
            let milestones = self.plan.get_milestones_for_objective(&objective.id);

            let achieved = milestones.iter().filter(|m| m.status == MilestoneStatus::Achieved).count();
            let in_progress = milestones.iter().filter(|m| m.status == MilestoneStatus::InProgress).count();
            let planned = milestones.iter().filter(|m| m.status == MilestoneStatus::Planned).count();

            output.push_str(&format!("- Milestones: {} achieved, {} in progress, {} planned\n",
                achieved, in_progress, planned
            ));

            // Key results progress (would come from goal completions in real implementation)
            output.push_str("- Key Results:\n");
            for kr in &objective.key_results {
                // Simulate progress percentage - in a real implementation this would come from actual data
                let progress = objective.progress;
                output.push_str(&format!("  - {} ({}%)\n", kr, progress));
            }

            output.push_str("\n");
        }

        // Recent activity
        output.push_str("## Recent Activity\n\n");

        // In a real implementation, this would show recently completed goals
        // and milestone status changes
        output.push_str("- No recent activity to report\n\n");

        // Upcoming deadlines
        output.push_str("## Upcoming Deadlines\n\n");

        let now = Utc::now();
        let upcoming_milestones: Vec<&Milestone> = self.plan.milestones.iter()
            .filter(|m| {
                m.status != MilestoneStatus::Achieved &&
                m.status != MilestoneStatus::Abandoned &&
                m.status != MilestoneStatus::Superseded &&
                m.target_date > now &&
                m.target_date < now + chrono::Duration::days(90)
            })
            .collect();

        if upcoming_milestones.is_empty() {
            output.push_str("- No upcoming deadlines in the next 90 days\n\n");
        } else {
            for milestone in upcoming_milestones {
                let days = milestone.target_date.signed_duration_since(now).num_days();
                output.push_str(&format!("- {} due in {} days ({})\n",
                    milestone.title,
                    days,
                    milestone.target_date.format("%Y-%m-%d")
                ));
            }
            output.push_str("\n");
        }

        Ok(output)
    }
}