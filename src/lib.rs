#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#![cfg_attr(test, allow(clippy::indexing_slicing))]

pub mod error;
pub mod limits;
pub mod loader;
pub mod model;
pub mod parse;
pub mod xmlutil;

#[cfg(feature = "gui")]
pub mod fonts;
#[cfg(feature = "gui")]
pub mod render;
#[cfg(feature = "gui")]
pub mod theme;
#[cfg(feature = "gui")]
pub mod ui;
#[cfg(feature = "gui")]
pub mod zoom_view;

pub use error::{LoadError, Result};
pub use model::Document;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub fn load_document(path: &Path, cancel: Arc<AtomicBool>) -> Result<Document> {
    let package = loader::load_package(path)?;
    parse::document::parse_document(package, cancel)
}
