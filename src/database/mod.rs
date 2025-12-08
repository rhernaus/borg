//! Database module for Borg
//!
//! This module implements a simple file-based database system
//! that provides persistent storage for the agent's data.

mod entities;
mod file_db;
mod manager;
mod models;

pub use file_db::{DatabaseError, DbResult, FileDb};
pub use manager::DatabaseManager;
pub use models::{Entity, Record};
