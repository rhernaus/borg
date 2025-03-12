use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::{debug, error, info};
use serde::Deserialize;
use tokio::sync::RwLock;
use thiserror::Error;

use super::models::{Entity, Record};

/// Result type for database operations
pub type DbResult<T> = Result<T, DatabaseError>;

/// Error type for database operations
#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("Entity not found with ID: {0}")]
    NotFound(String),

    #[error("Duplicate entity with ID: {0}")]
    DuplicateKey(String),

    #[error("Version conflict: expected {expected}, found {found}")]
    VersionConflict { expected: u64, found: u64 },

    #[error("Database I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Internal database error: {0}")]
    InternalError(String),
}

/// A file-based database for storing entities
pub struct FileDb<T: Entity + for<'a> Deserialize<'a> + Unpin> {
    /// Data directory where collection files are stored
    data_dir: PathBuf,

    /// Name of the collection (used as filename)
    collection_name: String,

    /// In-memory cache of records
    cache: Arc<RwLock<HashMap<T::Id, Record<T>>>>,

    /// Phantom data for the entity type
    _phantom: PhantomData<T>,
}

impl<T: Entity + for<'a> Deserialize<'a> + Unpin> FileDb<T> {
    /// Create a new file database
    pub async fn new(data_dir: impl AsRef<Path>, collection_name: &str) -> DbResult<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();

        // Create data directory if it doesn't exist
        fs::create_dir_all(&data_dir)
            .map_err(|e| DatabaseError::IoError(e))?;

        let db = Self {
            data_dir,
            collection_name: collection_name.to_string(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            _phantom: PhantomData,
        };

        // Load initial data
        db.load_all().await?;

        Ok(db)
    }

    /// Get the path to the collection file
    fn collection_path(&self) -> PathBuf {
        self.data_dir.join(format!("{}.json", self.collection_name))
    }

    /// Load all records from disk
    async fn load_all(&self) -> DbResult<()> {
        let path = self.collection_path();

        // If file doesn't exist, just return empty cache
        if !path.exists() {
            debug!("Collection file not found at {:?}, starting with empty database", path);
            return Ok(());
        }

        info!("Loading collection {} from {:?}", self.collection_name, path);

        // Open and read the file
        let file = File::open(&path)
            .map_err(|e| DatabaseError::IoError(e))?;

        let reader = BufReader::new(file);

        // Deserialize records from JSON
        let records: Vec<Record<T>> = serde_json::from_reader(reader)
            .map_err(|e| DatabaseError::SerializationError(e))?;

        // Update cache with loaded records
        let mut cache = self.cache.write().await;
        cache.clear();

        for record in records {
            cache.insert(record.id(), record);
        }

        info!("Successfully loaded {} records from {}", cache.len(), self.collection_name);
        Ok(())
    }

    /// Save all records to disk
    async fn save_all(&self) -> DbResult<()> {
        let path = self.collection_path();

        info!("Saving collection {} to {:?}", self.collection_name, path);

        // Create a temporary file for atomic write
        let temp_path = path.with_extension("tmp");

        // Get all records from cache
        let cache = self.cache.read().await;
        let records: Vec<Record<T>> = cache.values().cloned().collect();

        // Open a writer to the temporary file
        let file = File::create(&temp_path)
            .map_err(|e| DatabaseError::IoError(e))?;

        let writer = BufWriter::new(file);

        // Serialize records to JSON
        serde_json::to_writer_pretty(writer, &records)
            .map_err(|e| DatabaseError::SerializationError(e))?;

        // Atomically rename the temporary file to the actual file
        fs::rename(&temp_path, &path)
            .map_err(|e| DatabaseError::IoError(e))?;

        info!("Successfully saved {} records to {}", records.len(), self.collection_name);
        Ok(())
    }

    /// Get a record by ID
    pub async fn get(&self, id: &T::Id) -> DbResult<Record<T>> {
        let cache = self.cache.read().await;

        cache.get(id)
            .cloned()
            .ok_or_else(|| DatabaseError::NotFound(id.as_ref().to_string()))
    }

    /// Get all records
    pub async fn get_all(&self) -> DbResult<Vec<Record<T>>> {
        let cache = self.cache.read().await;

        let records = cache.values().cloned().collect();
        Ok(records)
    }

    /// Insert a new entity
    pub async fn insert(&self, entity: T) -> DbResult<Record<T>> {
        let mut cache = self.cache.write().await;

        let id = entity.id();

        // Ensure the ID doesn't already exist
        if cache.contains_key(&id) {
            return Err(DatabaseError::DuplicateKey(id.as_ref().to_string()));
        }

        // Create a new record
        let record = Record::new(entity);

        // Insert into cache
        cache.insert(id, record.clone());

        // Save changes
        drop(cache);
        self.save_all().await?;

        Ok(record)
    }

    /// Update an existing entity
    pub async fn update(&self, entity: T, expected_version: Option<u64>) -> DbResult<Record<T>> {
        let mut cache = self.cache.write().await;

        let id = entity.id();

        // Check if the entity exists
        let record = match cache.get_mut(&id) {
            Some(record) => record,
            None => return Err(DatabaseError::NotFound(id.as_ref().to_string())),
        };

        // Check version if provided
        if let Some(expected) = expected_version {
            if record.version != expected {
                return Err(DatabaseError::VersionConflict {
                    expected,
                    found: record.version,
                });
            }
        }

        // Update the record
        record.update(entity);

        let updated_record = record.clone();

        // Save changes
        drop(cache);
        self.save_all().await?;

        Ok(updated_record)
    }

    /// Delete a record by ID
    pub async fn delete(&self, id: &T::Id) -> DbResult<()> {
        let mut cache = self.cache.write().await;

        // Check if the entity exists
        if !cache.contains_key(id) {
            return Err(DatabaseError::NotFound(id.as_ref().to_string()));
        }

        // Remove the record
        cache.remove(id);

        // Save changes
        drop(cache);
        self.save_all().await?;

        Ok(())
    }

    /// Clear all records
    pub async fn clear(&self) -> DbResult<()> {
        let mut cache = self.cache.write().await;

        // Clear the cache
        cache.clear();

        // Save changes
        drop(cache);
        self.save_all().await?;

        Ok(())
    }
}