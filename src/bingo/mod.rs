pub mod card_image;
mod command;
mod error;
mod model;
mod store;
mod telegram;

pub use store::BingoStore;
pub use telegram::{AdminCache, answer_bingo, answer_callback, observe_message_users};

const _: fn(&model::Card) -> error::Result<Vec<u8>> = card_image::render_card_png;
