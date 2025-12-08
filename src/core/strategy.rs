use anyhow::Result;
use async_trait::async_trait;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::core::ethics::EthicsManager;
use crate::core::optimization::OptimizationGoal;

/// Types of actions the agent can take to achieve goals
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionType {
    /// Modify the codebase to improve it
    CodeImprovement,

    /// Make API calls to external services
    ApiCall,

    /// Research information on the web
    WebResearch,

    /// Execute system commands
    SystemCommand,

    /// Analyze data to derive insights
    DataAnalysis,
}

/// A concrete step in a plan with details about what to do
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionStep {
    /// Unique identifier for this step
    pub id: String,

    /// Description of what this step accomplishes
    pub description: String,

    /// The type of action to take
    pub action_type: ActionType,

    /// Dependencies on other steps (must be completed first)
    pub dependencies: Vec<String>,

    /// Specific parameters for this action
    pub parameters: HashMap<String, String>,

    /// Expected outcome when this step completes successfully
    pub expected_outcome: String,

    /// Whether this step requires explicit creator confirmation
    pub requires_confirmation: bool,
}

/// A complete plan to achieve a goal, consisting of multiple steps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Unique identifier for this plan
    pub id: String,

    /// The goal this plan aims to achieve
    pub goal_id: String,

    /// Ordered steps to execute
    pub steps: Vec<ActionStep>,

    /// Estimated success probability (0.0-1.0)
    pub success_probability: f64,

    /// Estimated resource requirements
    pub resource_estimate: HashMap<String, f64>,

    /// Overall strategy used to generate this plan
    pub strategy_name: String,

    /// Outputs from executed steps
    pub step_outputs: HashMap<String, String>,
}

impl Plan {
    /// Get the output of a previous step
    pub fn get_step_output(&self, key: &str) -> Option<String> {
        self.step_outputs.get(key).cloned()
    }

    /// Add output from a step
    pub fn add_step_output(&mut self, key: &str, value: &str) {
        self.step_outputs.insert(key.to_string(), value.to_string());
    }
}

/// Outcome of executing a plan or step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Whether the execution was successful
    pub success: bool,

    /// Detailed message about the result
    pub message: String,

    /// Any outputs or artifacts created
    pub outputs: HashMap<String, String>,

    /// Metrics about the execution
    pub metrics: HashMap<String, f64>,

    /// Log of steps taken during execution
    pub execution_log: Vec<String>,
}

/// Permission scope for actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionScope {
    /// Access to local file system
    LocalFileSystem(String),

    /// Network access to specific domains
    Network(Vec<String>),

    /// Access to specific API endpoints
    ApiEndpoint { url: String, methods: Vec<String> },

    /// Permission to run system commands
    SystemCommand(Vec<String>),
}

/// Permission for a specific action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPermission {
    /// The scope of this permission
    pub scope: PermissionScope,

    /// Whether explicit confirmation is required
    pub requires_confirmation: bool,

    /// Level of audit logging required
    pub audit_level: String,

    /// Expiry time for this permission, if any
    pub expiry: Option<chrono::DateTime<chrono::Utc>>,
}

/// Core trait for different strategy implementations
#[async_trait]
pub trait Strategy: Send + Sync {
    /// Get the name of this strategy
    fn name(&self) -> &str;

    /// Get the types of actions this strategy can perform
    fn action_types(&self) -> Vec<ActionType>;

    /// Evaluate how applicable this strategy is for a given goal
    /// Returns a score from 0.0 (not applicable) to 1.0 (perfect match)
    async fn evaluate_applicability(&self, goal: &OptimizationGoal) -> Result<f64>;

    /// Create a plan to achieve the given goal using this strategy
    async fn create_plan(&self, goal: &OptimizationGoal) -> Result<Plan>;

    /// Execute a plan or a specific step of a plan
    async fn execute(&self, plan: &Plan, step_id: Option<&str>) -> Result<ExecutionResult>;

    /// Check if this strategy has the required permissions
    fn check_permissions(&self, goal: &OptimizationGoal) -> Result<bool>;

    /// Get the required permissions for this strategy
    fn required_permissions(&self) -> Vec<ActionPermission>;
}

/// Manages selection and execution of different strategies
pub struct StrategyManager {
    /// Available strategies
    strategies: Vec<Box<dyn Strategy>>,

    /// Ethics manager for ethical validation
    #[allow(dead_code)]
    ethics_manager: Arc<Mutex<EthicsManager>>,

    /// Cached compatibility scores for goals and strategies
    compatibility_cache: HashMap<(String, String), f64>,
}

impl StrategyManager {
    /// Create a new strategy manager
    pub fn new(ethics_manager: Arc<Mutex<EthicsManager>>) -> Self {
        Self {
            strategies: Vec::new(),
            ethics_manager,
            compatibility_cache: HashMap::new(),
        }
    }

    /// Register a strategy with the manager
    pub fn register_strategy<S: Strategy + 'static>(&mut self, strategy: S) {
        info!("Registering strategy: {}", strategy.name());
        self.strategies.push(Box::new(strategy));
    }

    /// Get the best strategy for a goal
    pub async fn select_strategy(&mut self, goal: &OptimizationGoal) -> Result<&Box<dyn Strategy>> {
        info!("Selecting strategy for goal: {}", goal.id);

        let mut best_strategy = None;
        let mut best_score = 0.0;

        for strategy in &self.strategies {
            // Check if we have a cached score
            let cache_key = (goal.id.clone(), strategy.name().to_string());
            let score = match self.compatibility_cache.get(&cache_key) {
                Some(score) => *score,
                None => {
                    let score = strategy.evaluate_applicability(goal).await?;
                    self.compatibility_cache.insert(cache_key, score);
                    score
                }
            };

            if score > best_score {
                best_score = score;
                best_strategy = Some(strategy);
            }
        }

        match best_strategy {
            Some(strategy) if best_score > 0.0 => {
                info!(
                    "Selected strategy '{}' with score {:.2}",
                    strategy.name(),
                    best_score
                );
                Ok(strategy)
            }
            _ => {
                warn!("No suitable strategy found for goal: {}", goal.id);
                Err(anyhow::anyhow!("No suitable strategy found for goal"))
            }
        }
    }

    /// Create a plan for a goal using the best strategy
    pub async fn create_plan(&mut self, goal: &OptimizationGoal) -> Result<Plan> {
        let strategy = self.select_strategy(goal).await?;

        // Check permissions
        if !strategy.check_permissions(goal)? {
            return Err(anyhow::anyhow!(
                "Insufficient permissions for strategy: {}",
                strategy.name()
            ));
        }

        // Create the plan
        let plan = strategy.create_plan(goal).await?;

        // Perform ethical assessment of the plan
        self.assess_plan_ethics(&plan).await?;

        Ok(plan)
    }

    /// Execute a plan
    pub async fn execute_plan(&self, plan: &Plan) -> Result<ExecutionResult> {
        info!("Executing plan: {}", plan.id);

        // Find the strategy for this plan
        let strategy = self
            .strategies
            .iter()
            .find(|s| s.name() == plan.strategy_name)
            .ok_or_else(|| anyhow::anyhow!("Strategy not found: {}", plan.strategy_name))?;

        // Execute the plan
        strategy.execute(plan, None).await
    }

    /// Execute a specific step of a plan
    pub async fn execute_step(&self, plan: &Plan, step_id: &str) -> Result<ExecutionResult> {
        info!("Executing step {} of plan {}", step_id, plan.id);

        // Find the strategy for this plan
        let strategy = self
            .strategies
            .iter()
            .find(|s| s.name() == plan.strategy_name)
            .ok_or_else(|| anyhow::anyhow!("Strategy not found: {}", plan.strategy_name))?;

        // Execute the step
        strategy.execute(plan, Some(step_id)).await
    }

    /// Assess whether a plan is ethical
    async fn assess_plan_ethics(&self, plan: &Plan) -> Result<bool> {
        info!("Performing ethical assessment of plan: {}", plan.id);

        // Lock the ethics manager
        let mut ethics_manager = self.ethics_manager.lock().await;

        // Build a description of the plan for ethical assessment
        let plan_description = self.build_plan_description(plan)?;

        // Build a code representation of the planned changes
        let code_representation = self.build_plan_code_representation(plan)?;

        // Perform the ethical assessment
        let assessment =
            ethics_manager.assess_ethical_impact(&plan_description, &code_representation);

        // Log the assessment results
        info!(
            "Ethical assessment for plan {} completed with risk level: {:?}",
            plan.id, assessment.risk_level
        );

        if !assessment.is_approved {
            warn!("Ethical assessment rejected plan: {}", plan.id);
            warn!("Justification: {}", assessment.approval_justification);

            // Log the affected principles
            for (principle, impact) in &assessment.principle_impacts {
                warn!("Principle affected: {}: {}", principle, impact);
            }

            return Ok(false);
        }

        // If we have high risk but still approved, log warnings
        use crate::core::ethics::RiskLevel;
        if matches!(assessment.risk_level, RiskLevel::Medium | RiskLevel::High) {
            warn!(
                "Plan {} approved despite risk level: {:?}",
                plan.id, assessment.risk_level
            );
            warn!("Justification: {}", assessment.approval_justification);

            // Log the mitigations
            for mitigation in &assessment.mitigations {
                info!("Mitigation: {}", mitigation);
            }
        }

        Ok(assessment.is_approved)
    }

    /// Build a description of the plan for ethical assessment
    fn build_plan_description(&self, plan: &Plan) -> Result<String> {
        // Get the goal details (in a real system, this would be a lookup)
        let goal_description = match plan.goal_id.split('-').next() {
            Some("performance") => "Improve system performance",
            Some("security") => "Enhance system security",
            Some("readability") => "Improve code readability",
            Some("financial") => "Optimize financial operations",
            Some("complexity") => "Reduce code complexity",
            Some("test") => "Improve test coverage",
            _ => "Optimize system behavior",
        };

        // Describe the strategy
        let strategy = &plan.strategy_name;

        // Build a complete description
        let mut description = format!(
            "Plan to {} using {} strategy.\n",
            goal_description, strategy
        );

        // Add description of each step
        description.push_str("Steps:\n");
        for step in &plan.steps {
            description.push_str(&format!("- {}\n", step.description));
        }

        // Add resource estimates
        description.push_str("Resource estimates:\n");
        for (resource, value) in &plan.resource_estimate {
            description.push_str(&format!("- {}: {}\n", resource, value));
        }

        Ok(description)
    }

    /// Build a code representation of the planned changes
    fn build_plan_code_representation(&self, plan: &Plan) -> Result<String> {
        // In a real system, this might do a pre-analysis of what code would change
        // For now, we'll create a simplified representation based on the plan steps

        let mut code_representation = String::new();

        // Look at each step's parameters to infer code changes
        for step in &plan.steps {
            match step.action_type {
                ActionType::CodeImprovement => {
                    // Add basic code representation
                    code_representation.push_str("// Code improvement\n");

                    // Add any file targets from parameters
                    if let Some(file) = step.parameters.get("file") {
                        code_representation.push_str(&format!("// Target file: {}\n", file));
                    }

                    // Add any code parameter
                    if let Some(code) = step.parameters.get("code") {
                        code_representation.push_str(&format!(
                            "// Sample: {}\n",
                            if code.len() > 100 {
                                &code[0..100]
                            } else {
                                code
                            }
                        ));
                    }
                }
                ActionType::ApiCall => {
                    // Add API call representation
                    code_representation.push_str("// API Call\n");
                    if let Some(url) = step.parameters.get("url") {
                        code_representation.push_str(&format!("fetch(\"{}\")\n", url));
                    }
                }
                ActionType::SystemCommand => {
                    // Add system command representation
                    code_representation.push_str("// System Command\n");
                    if let Some(cmd) = step.parameters.get("command") {
                        code_representation.push_str(&format!("system(\"{}\")\n", cmd));
                    }
                }
                _ => {
                    // General representation for other action types
                    code_representation.push_str(&format!("// {}\n", step.description));
                }
            }
        }

        Ok(code_representation)
    }

    /// Get all registered strategies
    pub fn get_strategies(&self) -> Vec<&str> {
        self.strategies.iter().map(|s| s.name()).collect()
    }

    /// Get available action types across all strategies
    pub fn get_available_action_types(&self) -> Vec<ActionType> {
        let mut action_types = Vec::new();
        for strategy in &self.strategies {
            for action_type in strategy.action_types() {
                if !action_types.contains(&action_type) {
                    action_types.push(action_type);
                }
            }
        }
        action_types
    }
}
