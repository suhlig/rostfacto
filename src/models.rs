use chrono;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Retrospective {
    pub id: i32,
    pub title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Item {
    pub id: i32,
    pub retro_id: i32,
    pub text: String,
    pub category: Category,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: Status,
}

#[derive(Debug, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "category", rename_all = "UPPERCASE")]
pub enum Category {
    Good,
    Bad,
    Watch,
}

#[derive(Debug, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "status", rename_all = "UPPERCASE")]
pub enum Status {
    Created,
    Highlighted,
    Completed,
    Archived,
}

impl Default for Status {
    fn default() -> Self {
        Self::Created
    }
}

impl ToString for Category {
    fn to_string(&self) -> String {
        match self {
            Category::Good => "GOOD".to_string(),
            Category::Bad => "BAD".to_string(),
            Category::Watch => "WATCH".to_string(),
        }
    }
}
