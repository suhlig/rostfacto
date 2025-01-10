use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Retrospective {
    pub id: i32,
    pub title: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: ItemStatus,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct RetroItem {
    pub id: i32,
    pub retro_id: i32,
    pub text: String,
    pub category: ItemCategory,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "item_category", rename_all = "UPPERCASE")]
pub enum ItemCategory {
    Good,
    Bad,
    Watch,
}

#[derive(Debug, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "item_status", rename_all = "UPPERCASE")]
pub enum ItemStatus {
    Default,
    Highlighted,
    Completed,
}

impl Default for ItemStatus {
    fn default() -> Self {
        Self::Default
    }
}

impl ToString for ItemCategory {
    fn to_string(&self) -> String {
        match self {
            ItemCategory::Good => "GOOD".to_string(),
            ItemCategory::Bad => "BAD".to_string(),
            ItemCategory::Watch => "WATCH".to_string(),
        }
    }
}
