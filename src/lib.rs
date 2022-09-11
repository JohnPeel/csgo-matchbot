#[warn(clippy::pedantic)]
pub mod helpers;
pub mod models;

use sqlx::{PgExecutor, Result};

pub use models::*;

pub async fn create_user<'e>(
    executor: impl PgExecutor<'e>,
    discord_id: DiscordId,
    steam_id: SteamId,
) -> Result<User> {
    sqlx::query_as("INSERT INTO users (discord_id, steam_id) VALUES ($1, $2) RETURNING *")
        .bind(discord_id)
        .bind(steam_id)
        .fetch_one(executor)
        .await
}

pub async fn get_user_by_discord_id<'e>(
    executor: impl PgExecutor<'e>,
    discord_id: DiscordId,
) -> Result<Option<User>> {
    sqlx::query_as("SELECT * FROM users WHERE discord_id = $1")
        .bind(discord_id)
        .fetch_optional(executor)
        .await
}

pub async fn create_match<'e>(executor: impl PgExecutor<'e>, new_match: Match) -> Result<Match> {
    sqlx::query_as("INSERT INTO matches (team_one_role_id, team_one_name, team_two_role_id, team_two_name, note, series_type, date_added, match_state, scheduled_time_str) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING *")
        .bind(new_match.team_one_role_id)
        .bind(new_match.team_one_name)
        .bind(new_match.team_two_role_id)
        .bind(new_match.team_two_name)
        .bind(new_match.note)
        .bind(new_match.series_type)
        .bind(new_match.date_added)
        .bind(new_match.match_state)
        .bind(new_match.scheduled_time_str)
        .fetch_one(executor)
        .await
}

pub async fn get_match<'e>(executor: impl PgExecutor<'e>, match_id: i32) -> Result<Option<Match>> {
    sqlx::query_as("SELECT * FROM matches WHERE id = $1")
        .bind(match_id)
        .fetch_optional(executor)
        .await
}

pub async fn get_matches<'e>(
    executor: impl PgExecutor<'e>,
    limit: i64,
    state: MatchState,
) -> Result<Vec<Match>> {
    sqlx::query_as("SELECT * FROM matches WHERE match_state = $1 LIMIT $2")
        .bind(state)
        .bind(limit)
        .fetch_all(executor)
        .await
}

pub async fn get_next_team_match<'e>(
    executor: impl PgExecutor<'e>,
    team_role_id: i64,
) -> Result<Option<Match>> {
    sqlx::query_as("SELECT * FROM matches WHERE (team_one_role_id = $1 OR team_two_role_id = $1) AND match_state = $2 ORDER BY id LIMIT 1")
        .bind(team_role_id)
        .bind(MatchState::Entered)
        .fetch_optional(executor)
        .await
}

pub async fn update_match_schedule<'e>(
    executor: impl PgExecutor<'e>,
    match_id: i32,
    time_str: impl AsRef<str>,
) -> Result<bool> {
    let result = sqlx::query("UPDATE matches SET scheduled_time_str = $1 WHERE id = $2")
        .bind(time_str.as_ref())
        .bind(match_id)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn update_match_state<'e>(
    executor: impl PgExecutor<'e>,
    match_id: i32,
    state: MatchState,
) -> Result<bool> {
    let result = sqlx::query("UPDATE matches SET match_state = $1 WHERE id = $2")
        .bind(state)
        .bind(match_id)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn delete_match<'e>(executor: impl PgExecutor<'e>, match_id: i32) -> Result<bool> {
    let result = sqlx::query("DELETE FROM matches WHERE id = $1")
        .bind(match_id)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn create_match_setup_step<'e>(
    executor: impl PgExecutor<'e>,
    new_step: MatchSetupStep,
) -> Result<bool> {
    let result = sqlx::query("INSERT INTO match_setup_steps (match_id, step_type, team_role_id, map) VALUES ($1, $2, $3. $4)")
        .bind(new_step.match_id)
        .bind(new_step.step_type)
        .bind(new_step.team_role_id)
        .bind(new_step.map)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn get_match_setup_steps<'e>(
    executor: impl PgExecutor<'e>,
    match_id: i32,
) -> Result<Vec<MatchSetupStep>> {
    sqlx::query_as("SELECT * FROM match_setup_steps WHERE match_id = $1")
        .bind(match_id)
        .fetch_all(executor)
        .await
}

pub async fn create_series_map<'e>(
    executor: impl PgExecutor<'e>,
    new_series_map: SeriesMap,
) -> Result<bool> {
    let result = sqlx::query("INSERT INTO series_maps (match_id, map, picked_by_role_id, start_attack_team_role_id, start_defense_team_role_id) VALUES ($1, $2, $3, $4, $5)")
        .bind(new_series_map.match_id)
        .bind(new_series_map.map)
        .bind(new_series_map.picked_by_role_id)
        .bind(new_series_map.start_attack_team_role_id)
        .bind(new_series_map.start_defense_team_role_id)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn get_map_pool<'e>(executor: impl PgExecutor<'e>) -> Result<Vec<Map>> {
    sqlx::query_as("SELECT * FROM maps")
        .fetch_all(executor)
        .await
}

pub async fn get_match_servers<'e>(executor: impl PgExecutor<'e>) -> Result<Vec<MatchServer>> {
    sqlx::query_as("SELECT * FROM match_servers")
        .fetch_all(executor)
        .await
}

pub async fn get_fresh_token<'e>(executor: impl PgExecutor<'e>) -> Result<Option<GsltToken>> {
    sqlx::query_as("SELECT * FROM gslt_tokens WHERE in_use = false LIMIT 1")
        .fetch_optional(executor)
        .await
}

pub async fn update_token<'e>(executor: impl PgExecutor<'e>, token: &GsltToken) -> Result<bool> {
    let result = sqlx::query("UPDATE gslt_tokens SET in_use = $1 WHERE token = $2 RETURNING *")
        .bind(token.in_use)
        .bind(&token.token)
        .execute(executor)
        .await?;
    Ok(result.rows_affected() == 1)
}
