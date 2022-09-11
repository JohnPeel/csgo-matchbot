use displaydoc::Display;
use thiserror::Error;

#[derive(Debug, Display, Error)]
pub enum Error {
    /// Environment variable: {0}
    DotEnv(#[from] dotenvy::Error),
    /// Serenity error: {0}
    Serenity(#[from] serenity::Error),
    /// Database error: {0}
    Sqlx(#[from] sqlx::Error),

    /// Parsing error: {0}
    ParseIntError(#[from] std::num::ParseIntError),

    /// Unknown command: {0}
    UnknownCommand(String),
    /// Missing from context: {0}
    MissingFromContext(&'static str),
    /// Interaction has no guild
    InteractionHasNoGuild,

    /// Other error: {0}
    Other(#[from] anyhow::Error),
}
