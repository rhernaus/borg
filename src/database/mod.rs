//! Database module for Borg
//!
//! This module implements a simple file-based database system
//! that provides persistent storage for the agent's data.
//! It also provides a MongoDB-based implementation for cloud storage.

mod entities;
mod file_db;
mod manager;
mod models;
mod mongo_db;

pub use entities::StrategicPlanEntity;
pub use file_db::{DatabaseError, DbResult, FileDb};
pub use manager::DatabaseManager;
pub use models::{Entity, Record};
pub use mongo_db::MongoDb;
