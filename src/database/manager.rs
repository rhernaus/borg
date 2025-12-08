use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use log::info;
use serde::Deserialize;

use crate::core::config::Config;
use crate::core::optimization::OptimizationGoal;
use crate::database::{DbResult, Entity, FileDb, Record};

/// Database Manager coordinates access to all database collections
pub struct DatabaseManager {
    /// Base directory for all database files
    #[allow(dead_code)]
    data_dir: PathBuf,

    /// Database for optimization goals
    goals_db: Arc<dyn DatabaseInterface<OptimizationGoal>>,
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

impl DatabaseManager {
    /// Create a new database manager
    pub async fn new(data_dir: impl AsRef<Path>, _config: &Config) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();

        info!(
            "Initializing file-based database manager with data directory: {:?}",
            data_dir
        );

        // Create database for optimization goals
        let goals_db = FileDb::new(&data_dir, "optimization_goals")
            .await
            .context("Failed to create optimization goals database")?;

        Ok(Self {
            data_dir,
            goals_db: Arc::new(goals_db),
        })
    }

    /// Get the optimization goals database
    pub fn goals(&self) -> Arc<dyn DatabaseInterface<OptimizationGoal>> {
        self.goals_db.clone()
    }
}
