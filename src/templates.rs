use crate::auth::AuthUser;
use crate::models::{Item, Retrospective, Status};
use askama::Template;

#[derive(Template)]
#[template(path = "item_card.html")]
pub struct ItemCardTemplate {
    pub item: Item,
    pub is_admin: bool,
}

#[derive(Template)]
#[template(path = "archive_modal.html")]
pub struct ArchiveModalTemplate {
    pub item: Item,
}

#[derive(Template)]
#[template(path = "new_retro.html")]
pub struct NewRetroTemplate {
    pub is_admin: bool,
    pub teams: Vec<GitHubTeam>,
    pub demo_mode: bool,
    pub user: AuthUser,
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
    pub show_archive_modal: bool,
    pub is_admin: bool,
    pub user: Option<AuthUser>,
    pub demo_mode: bool,
}

pub struct GitHubTeam {
    pub slug: String,
    pub name: String,
}
