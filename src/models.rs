use core::str::FromStr;
use std::fmt;

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Type};
use time::OffsetDateTime;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, Type,
)]
#[repr(transparent)]
pub struct DiscordId(i64);

impl From<u64> for DiscordId {
    fn from(id: u64) -> Self {
        Self(id as i64)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, Type,
)]
#[repr(transparent)]
pub struct SteamId(i64);

impl From<steamid::SteamId> for SteamId {
    fn from(steam_id: steamid::SteamId) -> Self {
        Self(u64::from(steam_id) as i64)
    }
}

impl From<SteamId> for steamid::SteamId {
    fn from(steam_id: SteamId) -> Self {
        Self::new(steam_id.0 as u64).unwrap()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, FromRow,
)]
pub struct User {
    pub id: i32,
    pub discord_id: DiscordId,
    pub steam_id: SteamId,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, Type,
)]
#[sqlx(rename_all = "lowercase")]
pub enum SeriesType {
    Bo1,
    Bo3,
    Bo5,
}

impl fmt::Display for SeriesType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bo1 => write!(f, "Bo1"),
            Self::Bo3 => write!(f, "Bo3"),
            Self::Bo5 => write!(f, "Bo5"),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, Type,
)]
#[sqlx(rename_all = "lowercase")]
pub enum StepType {
    Veto,
    Pick,
}

impl fmt::Display for StepType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Veto => write!(f, "Veto"),
            Self::Pick => write!(f, "Pick"),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, Type,
)]
#[sqlx(rename_all = "lowercase")]
pub enum MatchState {
    Entered,
    Scheduled,
    Completed,
}

impl fmt::Display for MatchState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entered => write!(f, "Entered"),
            Self::Scheduled => write!(f, "Scheduled"),
            Self::Completed => write!(f, "Completed"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, FromRow)]
pub struct Match {
    pub id: Option<i32>,
    pub team_one_role_id: i64,
    pub team_one_name: String,
    pub team_two_role_id: i64,
    pub team_two_name: String,
    pub note: Option<String>,
    pub series_type: SeriesType,
    pub date_added: OffsetDateTime,
    pub match_state: MatchState,
    pub scheduled_time_str: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, FromRow)]
pub struct MatchSetupStep {
    pub id: Option<i32>,
    pub match_id: i32,
    pub step_type: StepType,
    pub team_role_id: i64,
    pub map: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, FromRow)]
pub struct SeriesMap {
    pub id: Option<i32>,
    pub match_id: i32,
    pub map: String,
    pub picked_by_role_id: i64,
    pub start_attack_team_role_id: Option<i64>,
    pub start_defense_team_role_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, FromRow)]
pub struct MatchServer {
    pub region_label: String,
    pub server_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, FromRow)]
pub struct GsltToken {
    pub token: String,
    pub in_use: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, FromRow)]
pub struct Map {
    pub name: String,
}

impl FromStr for SeriesType {
    type Err = ();
    fn from_str(input: &str) -> Result<SeriesType, Self::Err> {
        Ok(match input {
            "bo1" => SeriesType::Bo1,
            "bo3" => SeriesType::Bo3,
            "bo5" => SeriesType::Bo5,
            _ => Err(())?,
        })
    }
}
