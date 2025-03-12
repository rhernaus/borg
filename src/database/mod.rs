//! Database module for Borg
//!
//! This module implements a simple file-based database system
//! that provides persistent storage for the agent's data.
//! It also provides a MongoDB-based implementation for cloud storage.

mod file_db;
mod mongo_db;
mod models;
mod entities;
mod manager;

pub use file_db::{FileDb, DbResult, DatabaseError};
pub use mongo_db::MongoDb;
pub use models::{Entity, Record};
pub use entities::StrategicPlanEntity;
pub use manager::DatabaseManager;