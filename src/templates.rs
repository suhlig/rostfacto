use crate::auth::AuthUser;
use crate::models::{ActionItem, Archive, Category, Item, Retrospective, Status};
use askama::Template;

#[derive(Template)]
#[template(path = "item_card.html")]
pub struct ItemCardTemplate {
    pub item: Item,
    pub error_message: Option<String>,
}

#[derive(Template)]
#[template(path = "item_edit.html")]
pub struct ItemEditTemplate {
    pub item: Item,
}

#[derive(Template)]
#[template(path = "action_item.html")]
pub struct ActionItemTemplate {
    pub action_item: ActionItem,
}

#[derive(Template)]
#[template(path = "action_item_edit.html")]
pub struct ActionItemEditTemplate {
    pub action_item: ActionItem,
}

#[derive(Template)]
#[template(path = "archive_modal.html")]
pub struct ArchiveModalTemplate {
    pub item: Item,
    pub error_message: Option<String>,
}

#[derive(Template)]
#[template(path = "new_retro.html")]
pub struct NewRetroTemplate {
    pub is_admin: bool,
    pub teams: Vec<GitHubTeam>,
    pub demo_mode: bool,
    pub user: Option<AuthUser>,
}

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeTemplate {
    pub user: Option<AuthUser>,
    pub demo_mode: bool,
}

#[derive(Template)]
#[template(path = "retros.html")]
pub struct RetrosTemplate {
    pub retros: Vec<Retrospective>,
    pub is_admin: bool,
    pub user: Option<AuthUser>,
    pub demo_mode: bool,
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    pub code: &'static str,
    pub message: String,
    pub demo_mode: bool,
}

#[derive(Template)]
#[template(path = "retro.html")]
pub struct RetroTemplate {
    pub retro: Retrospective,
    pub good_items: Vec<Item>,
    pub bad_items: Vec<Item>,
    pub watch_items: Vec<Item>,
    pub action_items: Vec<ActionItem>,
    pub show_archive_modal: bool,
    pub is_admin: bool,
    pub user: Option<AuthUser>,
    pub demo_mode: bool,
    pub error_message: Option<String>,
    pub can_archive: bool,
}

#[derive(Template)]
#[template(path = "archives.html")]
pub struct ArchivesTemplate {
    pub retro: Retrospective,
    pub archives: Vec<ArchiveListEntry>,
    pub is_admin: bool,
    pub user: Option<AuthUser>,
    pub demo_mode: bool,
    pub can_archive: bool,
}

pub struct ArchiveListEntry {
    pub archive: Archive,
    pub items_count: i64,
    pub action_items_count: i64,
}

#[derive(Template)]
#[template(path = "archive.html")]
pub struct ArchiveTemplate {
    pub retro: Retrospective,
    pub archive: Archive,
    pub good_items: Vec<Item>,
    pub bad_items: Vec<Item>,
    pub watch_items: Vec<Item>,
    pub action_items: Vec<ActionItem>,
    pub is_admin: bool,
    pub user: Option<AuthUser>,
    pub demo_mode: bool,
    pub can_archive: bool,
}

pub struct GitHubTeam {
    pub slug: String,
}
