use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::fmt::Display;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Retrospective {
    pub id: i32,
    pub title: String,
    pub slug: String,
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

#[derive(Debug, Default, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "status", rename_all = "UPPERCASE")]
pub enum Status {
    #[default]
    Created,
    Highlighted,
    Completed,
    Archived,
}

impl Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Category::Good => write!(f, "GOOD"),
            Category::Bad => write!(f, "BAD"),
            Category::Watch => write!(f, "WATCH"),
        }
    }
}
