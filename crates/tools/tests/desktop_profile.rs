//! Exercise the production profile REGF reader/writer without a running kernel.
extern crate alloc;
#[path = "../../registry/src/registry/desktop_profile.rs"]
pub mod desktop_profile;
#[path = "../../registry/src/registry/hive.rs"]
pub mod hive;
#[path = "../../registry/src/registry/hive_writer.rs"]
pub mod hive_writer;
