use crate::models::{Item, Retrospective, Status};
use askama::Template;

#[derive(Template)]
#[template(path = "item_card.html")]
pub struct ItemCardTemplate {
    pub item: Item,
}

#[derive(Template)]
#[template(path = "archive_modal.html")]
pub struct ArchiveModalTemplate {
    pub item: Item,
}

#[derive(Template)]
#[template(path = "new_retro.html")]
pub struct NewRetroTemplate {}

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeTemplate;

#[derive(Template)]
#[template(path = "retros.html")]
pub struct RetrosTemplate {
    pub retros: Vec<Retrospective>,
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    pub code: &'static str,
    pub message: String,
}

#[derive(Template)]
#[template(path = "retro.html")]
pub struct RetroTemplate {
    pub retro: Retrospective,
    pub good_items: Vec<Item>,
    pub bad_items: Vec<Item>,
    pub watch_items: Vec<Item>,
    pub show_archive_modal: bool,
}
