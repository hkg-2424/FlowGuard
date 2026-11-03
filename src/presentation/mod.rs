pub mod console;
pub mod truncate;

pub use truncate::{truncate_str, truncate_to_graphemes, truncate_to_width, DEFAULT_ELLIPSIS};
