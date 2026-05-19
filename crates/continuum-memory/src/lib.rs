//! Persistent agent memory, backed by SQLite.
//!
//! SQLite has a single writer, so all access funnels through one writer actor
//! running on its own OS thread. Callers hold a cheap, clonable [`Memory`]
//! handle and talk to the actor over channels.

mod schema;
mod store;

pub use store::Memory;
