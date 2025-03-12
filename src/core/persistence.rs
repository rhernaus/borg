use anyhow::{Context, Result};
use log::{error, info, warn};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::core::optimization::{OptimizationGoal, OptimizationManager};

/// Handles persisting optimization goals to disk and loading them back
pub struct PersistenceManager {
    /// The directory where goal data will be stored
    data_dir: PathBuf,

    /// The filename for the goals file
    goals_filename: String,
}

impl PersistenceManager {
    /// Create a new persistence manager
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();

        // Create the data directory if it doesn't exist
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data directory: {:?}", data_dir))?;

        Ok(Self {
            data_dir,
            goals_filename: "optimization_goals.json".to_string(),
        })
    }

    /// Get the full path to the goals file
    fn goals_file_path(&self) -> PathBuf {
        self.data_dir.join(&self.goals_filename)
    }

    /// Save optimization goals to disk
    pub fn save_goals(&self, goals: &[OptimizationGoal]) -> Result<()> {
        let file_path = self.goals_file_path();
        info!("Saving {} optimization goals to {:?}", goals.len(), file_path);

        // Ensure the data directory exists before writing
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("Failed to create data directory: {:?}", self.data_dir))?;

        // Create a temporary file path for atomic write
        let temp_path = file_path.with_extension("tmp");

        // Open a buffered writer to the temporary file
        let file = File::create(&temp_path)
            .with_context(|| format!("Failed to create temporary goals file: {:?}", temp_path))?;
        let writer = BufWriter::new(file);

        // Serialize goals to JSON
        serde_json::to_writer_pretty(writer, goals)
            .with_context(|| "Failed to serialize goals to JSON")?;

        // Atomically rename the temporary file to the actual file
        // This prevents corruption if the process is interrupted during writing
        fs::rename(&temp_path, &file_path)
            .with_context(|| format!("Failed to rename temporary file to {:?}", file_path))?;

        info!("Successfully saved optimization goals to disk");
        Ok(())
    }

    /// Load optimization goals from disk
    pub fn load_goals(&self) -> Result<Vec<OptimizationGoal>> {
        let file_path = self.goals_file_path();

        // Ensure the data directory exists
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("Failed to create data directory: {:?}", self.data_dir))?;

        // If the file doesn't exist, return an empty vector
        if !file_path.exists() {
            info!("No goals file found at {:?}, starting with empty goals", file_path);
            return Ok(Vec::new());
        }

        info!("Loading optimization goals from {:?}", file_path);

        // Open a buffered reader for the file
        let file = File::open(&file_path)
            .with_context(|| format!("Failed to open goals file: {:?}", file_path))?;
        let reader = BufReader::new(file);

        // Deserialize goals from JSON
        let goals: Vec<OptimizationGoal> = serde_json::from_reader(reader)
            .with_context(|| "Failed to deserialize goals from JSON")?;

        info!("Successfully loaded {} optimization goals from disk", goals.len());
        Ok(goals)
    }

    /// Save the current state of the optimization manager
    pub async fn save_optimization_manager(&self, optimization_manager: &Arc<Mutex<OptimizationManager>>) -> Result<()> {
        let manager = optimization_manager.lock().await;
        let goals = manager.get_all_goals().to_vec();
        self.save_goals(&goals)
    }

    /// Load goals into the optimization manager
    pub async fn load_into_optimization_manager(&self, optimization_manager: &Arc<Mutex<OptimizationManager>>) -> Result<()> {
        let goals = self.load_goals()?;

        if !goals.is_empty() {
            let mut manager = optimization_manager.lock().await;

            // Clear existing goals and add the loaded ones
            manager.clear_goals();

            for goal in goals {
                manager.add_goal(goal);
            }

            // Update dependencies after loading all goals
            manager.update_goal_dependencies();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::optimization::{OptimizationGoal, OptimizationCategory, PriorityLevel};
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load_goals() -> Result<()> {
        // Create a temporary directory for testing
        let temp_dir = tempdir()?;
        let persistence_manager = PersistenceManager::new(temp_dir.path())?;

        // Create some test goals
        let goals = vec![
            {
                let mut goal = OptimizationGoal::new(
                    "test-001",
                    "Test Goal 1",
                    "This is a test goal",
                    OptimizationCategory::General,
                );
                goal.priority = PriorityLevel::High;
                goal
            },
            {
                let mut goal = OptimizationGoal::new(
                    "test-002",
                    "Test Goal 2",
                    "This is another test goal",
                    OptimizationCategory::Performance,
                );
                goal.priority = PriorityLevel::Medium;
                goal
            },
        ];

        // Save the goals
        persistence_manager.save_goals(&goals)?;

        // Load the goals
        let loaded_goals = persistence_manager.load_goals()?;

        // Verify loaded goals match original goals
        assert_eq!(loaded_goals.len(), goals.len());
        assert_eq!(loaded_goals[0].id, goals[0].id);
        assert_eq!(loaded_goals[0].title, goals[0].title);
        assert_eq!(loaded_goals[1].id, goals[1].id);
        assert_eq!(loaded_goals[1].priority, goals[1].priority);

        Ok(())
    }
}