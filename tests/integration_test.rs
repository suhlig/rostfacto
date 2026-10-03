//! WebDriver integration tests, split by feature area.
//!
//! The test functions live in the `integration_test/` directory. The `#[path]`
//! attributes are required because a crate root resolves its submodules
//! relative to `tests/` (the directory containing this file), not relative to a
//! directory named after the file.

#[path = "integration_test/action_items.rs"]
mod action_items;
#[path = "integration_test/archive.rs"]
mod archive;
#[path = "integration_test/cards.rs"]
mod cards;
#[path = "integration_test/common.rs"]
mod common;
#[path = "integration_test/home.rs"]
mod home;
#[path = "integration_test/participants.rs"]
mod participants;
#[path = "integration_test/retros.rs"]
mod retros;
#[path = "integration_test/sse.rs"]
mod sse;

mod test_helpers;
