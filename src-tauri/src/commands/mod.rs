pub mod ai;
pub mod devices;
pub mod meetings;
pub mod models;
pub mod organize;
pub mod recording;
pub mod settings;
pub mod speakers;

/// Commands return `Result<T, String>` so the UI gets a readable message.
pub type CmdResult<T> = Result<T, String>;

pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
