use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::hash::Hash;

/// Trait for database entity types
///
/// This trait defines the requirements for entities that can be stored
/// in the file database. It requires an ID field for identifying
/// and retrieving records.
pub trait Entity: Serialize + Clone + Debug + Send + Sync + 'static {
    /// Type of the entity's unique identifier
    type Id: AsRef<str> + Eq + Hash + Clone + Debug + Send + Sync + 'static;

    /// Get the unique identifier for this entity
    fn id(&self) -> Self::Id;
}

/// A record in the database, which wraps an entity with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: Entity + for<'a> Deserialize<'a>"))]
pub struct Record<T: Entity + for<'a> Deserialize<'a> + Unpin> {
    /// The entity being stored
    pub entity: T,
    /// When the record was created
    pub created_at: DateTime<Utc>,
    /// When the record was last updated
    pub updated_at: DateTime<Utc>,
    /// Version number for optimistic concurrency control
    pub version: u64,
}

impl<T: Entity + for<'a> Deserialize<'a> + Unpin> Record<T> {
    /// Create a new record with the given entity
    pub fn new(entity: T) -> Self {
        let now = Utc::now();
        Self {
            entity,
            created_at: now,
            updated_at: now,
            version: 1,
        }
    }

    /// Update the record with a new entity
    pub fn update(&mut self, entity: T) {
        self.entity = entity;
        self.updated_at = Utc::now();
        self.version += 1;
    }

    /// Get the entity's ID
    pub fn id(&self) -> T::Id {
        self.entity.id()
    }

    /// Get a reference to the entity
    pub fn entity(&self) -> &T {
        &self.entity
    }
}
