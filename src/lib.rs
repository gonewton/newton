pub mod cli;
pub mod core;
pub mod tools;
pub mod utils;

pub type Result<T> = std::result::Result<T, anyhow::Error>;