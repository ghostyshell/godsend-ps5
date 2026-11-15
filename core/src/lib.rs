//! Framework-agnostic engine for managing a jailbroken PS5 over FTP, elfldr
//! and the Ghostpad controller payload.
//!
//! Everything here works without a GUI so the same code backs the Tauri app
//! and the CLI.

pub mod aria2;
pub mod config;
pub mod convert;
pub mod debrid;
pub mod device;
pub mod doctor;
pub mod error;
pub mod fm;
pub mod ftp;
pub mod ghostpad;
pub mod homebrew;
pub mod install;
pub mod iso;
pub mod isobuild;
pub mod library;
pub mod pad;
pub mod payload;
pub mod pkg;
pub mod sfo;
pub mod sources;
pub mod split;
pub mod store;
pub mod transfer;

pub use config::{Config, Host};
pub use error::{Error, Result};
pub use library::GameEntry;
pub use transfer::{Job, JobManager, JobState};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Product name used for data directories and window titles.
pub const PRODUCT: &str = "GODsend PS5";
