use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use std::fmt::Display;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Retrospective {
    pub id: i32,
    pub title: String,
    pub slug: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub team_slug: String,
    pub created_by: i32,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Item {
    pub id: i32,
    pub retro_id: i32,
    pub text: String,
    pub category: Category,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub status: Status,
    pub author_id: i32,
    pub author_name: String,
    pub author_initials: String,
}

pub fn apply_author_initials(items: &mut [&mut Vec<Item>]) {
    let mut base_initial_counts = HashMap::new();
    let mut authors = HashMap::new();

    for item in items.iter().flat_map(|items| items.iter()) {
        authors
            .entry(item.author_id)
            .or_insert_with(|| item.author_name.clone());
    }
    for name in authors.values() {
        *base_initial_counts
            .entry(initials(name, false))
            .or_insert(0) += 1;
    }

    for item in items.iter_mut().flat_map(|items| items.iter_mut()) {
        let base = initials(&item.author_name, false);
        item.author_initials = initials(
            &item.author_name,
            base_initial_counts.get(&base).copied().unwrap_or(0) > 1,
        );
    }
}

fn initials(name: &str, disambiguate: bool) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let Some(first_name) = words.first() else {
        return "?".to_string();
    };
    let last_name = words.last().unwrap();

    if words.len() == 1 {
        return first_name
            .chars()
            .take(if disambiguate { 3 } else { 2 })
            .collect::<String>()
            .to_uppercase();
    }

    let last_chars = if disambiguate { 2 } else { 1 };
    first_name
        .chars()
        .take(1)
        .chain(last_name.chars().take(last_chars))
        .collect::<String>()
        .to_uppercase()
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

impl Category {
    /// URL/path segment used in HTMX endpoints (e.g. `/items/Good/{id}`).
    pub const fn url_segment(&self) -> &'static str {
        match self {
            Category::Good => "Good",
            Category::Bad => "Bad",
            Category::Watch => "Watch",
        }
    }

    /// Human-readable column label.
    pub const fn display_label(&self) -> &'static str {
        match self {
            Category::Good => "Good",
            Category::Bad => "Bad",
            Category::Watch => "Watch",
        }
    }

    /// CSS class suffix for the retro board column.
    pub const fn column_class(&self) -> &'static str {
        match self {
            Category::Good => "column-happy",
            Category::Bad => "column-sad",
            Category::Watch => "column-meh",
        }
    }

    /// Icon filename for the column header.
    pub const fn icon(&self) -> &'static str {
        match self {
            Category::Good => "happy.svg",
            Category::Bad => "sad.svg",
            Category::Watch => "meh.svg",
        }
    }

    /// DOM id for the list container of items in this column.
    pub const fn items_container_id(&self) -> &'static str {
        match self {
            Category::Good => "good-items",
            Category::Bad => "bad-items",
            Category::Watch => "watch-items",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn builds_initials_from_first_and_last_names() {
        assert_eq!(initials("Stefan Uhlig", false), "SU");
    }

    #[test]
    fn uses_two_last_name_characters_to_disambiguate() {
        assert_eq!(initials("Stefan Uhlig", true), "SUH");
    }

    #[test]
    fn falls_back_to_the_username_for_single_word_names() {
        assert_eq!(initials("suhlig", false), "SU");
    }
}
