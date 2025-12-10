//! High-performance chat protocol core library with lock-free data structures

pub mod batching;
pub mod error;
pub mod message_cache;
pub mod protocol;
pub mod transport_layer;
pub mod utils;

pub use error::{ApplicationError, Result};
pub use message_cache::MessageCache;
pub use protocol::{ClientMessage, ServerMessage, SharedServerMessage};
