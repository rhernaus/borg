use std::marker::PhantomData;

use futures_util::TryStreamExt;
use log::info;
use mongodb::bson::doc;
use mongodb::options::ClientOptions;
use mongodb::{Client, Collection, Database};
use serde::Deserialize;

use super::file_db::DatabaseError;
use super::models::{Entity, Record};

/// Result type for database operations
pub type DbResult<T> = Result<T, DatabaseError>;

/// A MongoDB-based database for storing entities
pub struct MongoDb<T: Entity + for<'a> Deserialize<'a> + Unpin> {
    /// MongoDB client
    client: Client,

    /// MongoDB database
    database: Database,

    /// MongoDB collection
    collection: Collection<Record<T>>,

    /// Phantom data for the entity type
    _phantom: PhantomData<T>,
}

impl<T: Entity + for<'a> Deserialize<'a> + Unpin> MongoDb<T> {
    /// Create a new MongoDB database
    pub async fn new(
        connection_string: &str,
        database_name: &str,
        collection_name: &str,
    ) -> DbResult<Self> {
        // Parse connection string and create client options
        let client_options = ClientOptions::parse(connection_string).await.map_err(|e| {
            DatabaseError::InternalError(format!(
                "Failed to parse MongoDB connection string: {}",
                e
            ))
        })?;

        // Create client
        let client = Client::with_options(client_options).map_err(|e| {
            DatabaseError::InternalError(format!("Failed to create MongoDB client: {}", e))
        })?;

        // Get database and collection
        let database = client.database(database_name);
        let collection = database.collection::<Record<T>>(collection_name);

        info!(
            "Connected to MongoDB database: {}, collection: {}",
            database_name, collection_name
        );

        Ok(Self {
            client,
            database,
            collection,
            _phantom: PhantomData,
        })
    }

    /// Get a record by ID
    pub async fn get(&self, id: &T::Id) -> DbResult<Record<T>> {
        let filter = doc! { "entity.id": id.as_ref() };

        match self.collection.find_one(filter).await {
            Ok(Some(record)) => Ok(record),
            Ok(None) => Err(DatabaseError::NotFound(id.as_ref().to_string())),
            Err(e) => Err(DatabaseError::InternalError(format!(
                "MongoDB error: {}",
                e
            ))),
        }
    }

    /// Get all records
    pub async fn get_all(&self) -> DbResult<Vec<Record<T>>> {
        let cursor = self
            .collection
            .find(doc! {})
            .await
            .map_err(|e| DatabaseError::InternalError(format!("MongoDB error: {}", e)))?;

        let records = cursor
            .try_collect()
            .await
            .map_err(|e| DatabaseError::InternalError(format!("MongoDB error: {}", e)))?;

        Ok(records)
    }

    /// Insert a new entity
    pub async fn insert(&self, entity: T) -> DbResult<Record<T>> {
        let id = entity.id();

        // Check if entity already exists
        let filter = doc! { "entity.id": id.as_ref() };
        let exists = self
            .collection
            .find_one(filter.clone())
            .await
            .map_err(|e| DatabaseError::InternalError(format!("MongoDB error: {}", e)))?;

        if exists.is_some() {
            return Err(DatabaseError::DuplicateKey(id.as_ref().to_string()));
        }

        // Create a new record
        let record = Record::new(entity);

        // Insert into MongoDB
        self.collection
            .insert_one(&record)
            .await
            .map_err(|e| DatabaseError::InternalError(format!("MongoDB error: {}", e)))?;

        info!("Inserted record with ID: {}", id.as_ref());

        Ok(record)
    }

    /// Update an existing entity
    pub async fn update(&self, entity: T, expected_version: Option<u64>) -> DbResult<Record<T>> {
        let id = entity.id();

        // Find the existing record
        let filter = doc! { "entity.id": id.as_ref() };
        let existing = self
            .collection
            .find_one(filter.clone())
            .await
            .map_err(|e| DatabaseError::InternalError(format!("MongoDB error: {}", e)))?;

        let mut existing = match existing {
            Some(record) => record,
            None => return Err(DatabaseError::NotFound(id.as_ref().to_string())),
        };

        // Check version if provided
        if let Some(expected) = expected_version {
            if existing.version != expected {
                return Err(DatabaseError::VersionConflict {
                    expected,
                    found: existing.version,
                });
            }
        }

        // Update the record
        existing.update(entity);

        // Update in MongoDB
        let update = doc! { "$set": mongodb::bson::to_document(&existing)
        .map_err(|e| DatabaseError::InternalError(format!("MongoDB serialization error: {}", e)))? };

        self.collection
            .update_one(filter, update)
            .await
            .map_err(|e| DatabaseError::InternalError(format!("MongoDB error: {}", e)))?;

        info!("Updated record with ID: {}", id.as_ref());

        Ok(existing)
    }

    /// Delete a record by ID
    pub async fn delete(&self, id: &T::Id) -> DbResult<()> {
        let filter = doc! { "entity.id": id.as_ref() };

        let result = self
            .collection
            .delete_one(filter)
            .await
            .map_err(|e| DatabaseError::InternalError(format!("MongoDB error: {}", e)))?;

        if result.deleted_count == 0 {
            return Err(DatabaseError::NotFound(id.as_ref().to_string()));
        }

        info!("Deleted record with ID: {}", id.as_ref());

        Ok(())
    }

    /// Clear all records
    pub async fn clear(&self) -> DbResult<()> {
        self.collection
            .delete_many(doc! {})
            .await
            .map_err(|e| DatabaseError::InternalError(format!("MongoDB error: {}", e)))?;

        info!("Cleared all records from collection");

        Ok(())
    }
}
