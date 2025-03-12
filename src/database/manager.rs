use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use log::info;
use serde::Deserialize;

use crate::core::planning::{Milestone, StrategicObjective, StrategicPlan};
use crate::core::optimization::OptimizationGoal;
use crate::core::config::Config;
use crate::database::{FileDb, MongoDb, DbResult, Entity, StrategicPlanEntity, Record};

/// Database type enum to distinguish between file-based and MongoDB storage
#[derive(Debug, Clone, Copy)]
pub enum DatabaseType {
    /// File-based storage
    File,
    /// MongoDB storage
    Mongo,
}

/// Database Manager coordinates access to all database collections
pub struct DatabaseManager {
    /// Base directory for all database files (used for file-based storage)
    data_dir: PathBuf,

    /// Database for strategic objectives
    objectives_db: Arc<dyn DatabaseInterface<StrategicObjective>>,

    /// Database for milestones
    milestones_db: Arc<dyn DatabaseInterface<Milestone>>,

    /// Database for optimization goals
    goals_db: Arc<dyn DatabaseInterface<OptimizationGoal>>,

    /// Database for strategic plans
    plans_db: Arc<dyn DatabaseInterface<StrategicPlanEntity>>,

    /// Type of database being used
    db_type: DatabaseType,
}

/// Trait for database operations
#[async_trait::async_trait]
pub trait DatabaseInterface<T: Entity + for<'a> Deserialize<'a> + Unpin>: Send + Sync {
    /// Get a record by ID
    async fn get(&self, id: &T::Id) -> DbResult<Record<T>>;

    /// Get all records
    async fn get_all(&self) -> DbResult<Vec<Record<T>>>;

    /// Insert a new entity
    async fn insert(&self, entity: T) -> DbResult<Record<T>>;

    /// Update an existing entity
    async fn update(&self, entity: T, expected_version: Option<u64>) -> DbResult<Record<T>>;

    /// Delete a record by ID
    async fn delete(&self, id: &T::Id) -> DbResult<()>;

    /// Clear all records
    async fn clear(&self) -> DbResult<()>;
}

// Implement DatabaseInterface for FileDb
#[async_trait::async_trait]
impl<T: Entity + for<'a> Deserialize<'a> + Unpin> DatabaseInterface<T> for FileDb<T> {
    async fn get(&self, id: &T::Id) -> DbResult<Record<T>> {
        self.get(id).await
    }

    async fn get_all(&self) -> DbResult<Vec<Record<T>>> {
        self.get_all().await
    }

    async fn insert(&self, entity: T) -> DbResult<Record<T>> {
        self.insert(entity).await
    }

    async fn update(&self, entity: T, expected_version: Option<u64>) -> DbResult<Record<T>> {
        self.update(entity, expected_version).await
    }

    async fn delete(&self, id: &T::Id) -> DbResult<()> {
        self.delete(id).await
    }

    async fn clear(&self) -> DbResult<()> {
        self.clear().await
    }
}

// Implement DatabaseInterface for MongoDb
#[async_trait::async_trait]
impl<T: Entity + for<'a> Deserialize<'a> + Unpin> DatabaseInterface<T> for MongoDb<T> {
    async fn get(&self, id: &T::Id) -> DbResult<Record<T>> {
        self.get(id).await
    }

    async fn get_all(&self) -> DbResult<Vec<Record<T>>> {
        self.get_all().await
    }

    async fn insert(&self, entity: T) -> DbResult<Record<T>> {
        self.insert(entity).await
    }

    async fn update(&self, entity: T, expected_version: Option<u64>) -> DbResult<Record<T>> {
        self.update(entity, expected_version).await
    }

    async fn delete(&self, id: &T::Id) -> DbResult<()> {
        self.delete(id).await
    }

    async fn clear(&self) -> DbResult<()> {
        self.clear().await
    }
}

impl DatabaseManager {
    /// Create a new database manager
    pub async fn new(data_dir: impl AsRef<Path>, config: &Config) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();

        // Determine which database type to use based on configuration
        let db_type = if config.mongodb.enabled {
            info!("Using MongoDB for database storage");
            DatabaseType::Mongo
        } else {
            info!("Using file-based database storage");
            DatabaseType::File
        };

        match db_type {
            DatabaseType::File => {
                info!("Initializing file-based database manager with data directory: {:?}", data_dir);

                // Create databases for each collection
                let objectives_db = FileDb::new(&data_dir, "strategic_objectives").await
                    .context("Failed to create strategic objectives database")?;

                let milestones_db = FileDb::new(&data_dir, "milestones").await
                    .context("Failed to create milestones database")?;

                let goals_db = FileDb::new(&data_dir, "optimization_goals").await
                    .context("Failed to create optimization goals database")?;

                let plans_db = FileDb::new(&data_dir, "strategic_plans").await
                    .context("Failed to create strategic plans database")?;

                Ok(Self {
                    data_dir,
                    objectives_db: Arc::new(objectives_db),
                    milestones_db: Arc::new(milestones_db),
                    goals_db: Arc::new(goals_db),
                    plans_db: Arc::new(plans_db),
                    db_type,
                })
            },
            DatabaseType::Mongo => {
                info!("Initializing MongoDB database manager with connection: {}",
                    config.mongodb.connection_string);

                // Create MongoDB databases for each collection
                let objectives_db = MongoDb::new(
                    &config.mongodb.connection_string,
                    &config.mongodb.database,
                    "strategic_objectives"
                ).await.context("Failed to create strategic objectives MongoDB database")?;

                let milestones_db = MongoDb::new(
                    &config.mongodb.connection_string,
                    &config.mongodb.database,
                    "milestones"
                ).await.context("Failed to create milestones MongoDB database")?;

                let goals_db = MongoDb::new(
                    &config.mongodb.connection_string,
                    &config.mongodb.database,
                    "optimization_goals"
                ).await.context("Failed to create optimization goals MongoDB database")?;

                let plans_db = MongoDb::new(
                    &config.mongodb.connection_string,
                    &config.mongodb.database,
                    "strategic_plans"
                ).await.context("Failed to create strategic plans MongoDB database")?;

                Ok(Self {
                    data_dir,
                    objectives_db: Arc::new(objectives_db),
                    milestones_db: Arc::new(milestones_db),
                    goals_db: Arc::new(goals_db),
                    plans_db: Arc::new(plans_db),
                    db_type,
                })
            }
        }
    }

    /// Get the strategic objectives database
    pub fn objectives(&self) -> Arc<dyn DatabaseInterface<StrategicObjective>> {
        self.objectives_db.clone()
    }

    /// Get the milestones database
    pub fn milestones(&self) -> Arc<dyn DatabaseInterface<Milestone>> {
        self.milestones_db.clone()
    }

    /// Get the optimization goals database
    pub fn goals(&self) -> Arc<dyn DatabaseInterface<OptimizationGoal>> {
        self.goals_db.clone()
    }

    /// Get the strategic plans database
    pub fn plans(&self) -> Arc<dyn DatabaseInterface<StrategicPlanEntity>> {
        self.plans_db.clone()
    }

    /// Get the current strategic plan if it exists
    pub async fn get_current_plan(&self) -> DbResult<Option<StrategicPlan>> {
        // Try to get the current plan
        match self.plans_db.get(&"current".to_string()).await {
            Ok(record) => Ok(Some(record.entity.plan)),
            Err(crate::database::DatabaseError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Save the current strategic plan
    pub async fn save_plan(&self, plan: StrategicPlan) -> DbResult<()> {
        let plan_entity = StrategicPlanEntity::new(plan);

        // Try to get the current plan first to see if it exists
        match self.plans_db.get(&"current".to_string()).await {
            Ok(record) => {
                // Update existing record
                self.plans_db.update(plan_entity, Some(record.version)).await?;
            }
            Err(crate::database::DatabaseError::NotFound(_)) => {
                // Insert new record
                self.plans_db.insert(plan_entity).await?;
            }
            Err(e) => return Err(e),
        }

        Ok(())
    }

    /// Save all objects in a strategic plan
    pub async fn save_full_plan(&self, plan: &StrategicPlan) -> Result<()> {
        // First save all objectives
        for objective in &plan.objectives {
            match self.objectives_db.get(&objective.id()).await {
                Ok(record) => {
                    self.objectives_db.update(objective.clone(), Some(record.version)).await
                        .context("Failed to update strategic objective")?;
                }
                Err(crate::database::DatabaseError::NotFound(_)) => {
                    self.objectives_db.insert(objective.clone()).await
                        .context("Failed to insert strategic objective")?;
                }
                Err(e) => return Err(e.into()),
            }
        }

        // Then save all milestones
        for milestone in &plan.milestones {
            match self.milestones_db.get(&milestone.id()).await {
                Ok(record) => {
                    self.milestones_db.update(milestone.clone(), Some(record.version)).await
                        .context("Failed to update milestone")?;
                }
                Err(crate::database::DatabaseError::NotFound(_)) => {
                    self.milestones_db.insert(milestone.clone()).await
                        .context("Failed to insert milestone")?;
                }
                Err(e) => return Err(e.into()),
            }
        }

        // Finally save the plan itself
        self.save_plan(plan.clone()).await
            .context("Failed to save strategic plan")?;

        Ok(())
    }

    /// Get the type of database being used
    pub fn database_type(&self) -> DatabaseType {
        self.db_type
    }
}