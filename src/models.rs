use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::schema::{match_setup_step, matches, series_map, users};
use crate::models::SeriesType::{Bo1, Bo3, Bo5};

pub use enums::*;

#[derive(Queryable)]
pub struct User {
    pub id: i32,
    pub discord_id: i64,
    pub steam_id: String,
}

#[derive(Insertable)]
#[table_name = "users"]
pub struct NewUser<'a> {
    pub discord_id: i64,
    pub steam_id: &'a str,
}

#[derive(Insertable)]
#[table_name = "matches"]
pub struct NewMatch<'a> {
    pub team_one_role_id: i64,
    pub team_one_name: &'a str,
    pub team_two_role_id: i64,
    pub team_two_name: &'a str,
    pub note: Option<&'a str>,
    pub series_type: &'a SeriesType,
    pub date_added: &'a NaiveDateTime,
    pub match_state: &'a MatchState,
}

#[derive(Queryable, Clone, Serialize, Deserialize)]
pub struct Match {
    pub id: i32,
    pub team_one_role_id: i64,
    pub team_one_name: String,
    pub team_two_role_id: i64,
    pub team_two_name: String,
    pub note: Option<String>,
    pub date_added: NaiveDateTime,
    pub match_state: MatchState,
    pub scheduled_time_str: Option<String>,
    pub series_type: SeriesType,
}

#[derive(Queryable, Clone, Serialize, Deserialize, Debug)]
pub struct MatchSetupStep {
    pub id: i32,
    pub match_id: i32,
    pub step_type: StepType,
    pub team_role_id: i64,
    pub map: Option<String>,
}

#[derive(Insertable, Clone)]
#[table_name = "match_setup_step"]
pub struct NewMatchSetupStep {
    pub match_id: i32,
    pub step_type: StepType,
    pub team_role_id: i64,
    pub map: Option<String>,
}

#[derive(Queryable, Clone, Serialize, Deserialize)]
pub struct SeriesMap {
    pub id: i32,
    pub match_id: i32,
    pub map: String,
    pub picked_by_role_id: i64,
    pub start_attack_team_role_id: Option<i64>,
    pub start_defense_team_role_id: Option<i64>,
}

#[derive(Insertable, Clone)]
#[table_name = "series_map"]
pub struct NewSeriesMap {
    pub match_id: i32,
    pub map: String,
    pub picked_by_role_id: i64,
    pub start_attack_team_role_id: Option<i64>,
    pub start_defense_team_role_id: Option<i64>,
}

#[derive(Queryable, Clone, Serialize, Deserialize)]
pub struct MatchServer {
    pub region_label: String,
    pub server_id: String,
}

#[derive(Queryable, Clone, Serialize, Deserialize)]
pub struct GsltToken {
    pub token: String,
    pub in_use: bool,
}

#[derive(Queryable, Clone, Serialize, Deserialize)]
pub struct Map {
    pub name: String,
}

mod enums {
    // The DbEnum derive macro has some clippy issues, and isn't something we can fix here.
    #![allow(clippy::from_over_into)]

    use diesel::types::VarChar;
    use diesel_enum::DbEnum;
    use serde::{Deserialize, Serialize};
    use std::fmt;

    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, AsExpression, FromSqlRow, DbEnum, Serialize, Deserialize,
    )]
    #[sql_type = "VarChar"]
    #[error_fn = "CustomError::not_found"]
    #[error_type = "CustomError"]
    pub enum SeriesType {
        Bo1,
        Bo3,
        Bo5,
    }

    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, AsExpression, FromSqlRow, DbEnum, Serialize, Deserialize,
    )]
    #[sql_type = "VarChar"]
    #[error_fn = "CustomError::not_found"]
    #[error_type = "CustomError"]
    pub enum StepType {
        Veto,
        Pick,
    }

    impl fmt::Display for StepType {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}",
                match self {
                    StepType::Veto => "ban",
                    StepType::Pick => "pick",
                }
            )
        }
    }

    #[allow(clippy::from_over_into)]
    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, AsExpression, FromSqlRow, DbEnum, Serialize, Deserialize,
    )]
    #[sql_type = "VarChar"]
    #[error_fn = "CustomError::not_found"]
    #[error_type = "CustomError"]
    pub enum MatchState {
        Entered,
        Scheduled,
        Completed,
    }

    #[derive(Debug)]
    pub struct CustomError {
        pub msg: String,
        pub status: u16,
    }

    impl CustomError {
        fn not_found(msg: String) -> Self {
            Self { msg, status: 404 }
        }
    }
}

impl FromStr for SeriesType {
    type Err = ();
    fn from_str(input: &str) -> Result<SeriesType, Self::Err> {
        match input {
            "bo1" => Ok(Bo1),
            "bo3" => Ok(Bo3),
            "bo5" => Ok(Bo5),
            _ => Err(()),
        }
    }
}
