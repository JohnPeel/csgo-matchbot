#[warn(clippy::pedantic)]
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]
mod commands;
mod dathost_models;
mod error;
mod utils;

use std::env;
use std::str::FromStr;

use csgo_matchbot::models::{Match, SeriesType, StepType};
use error::Error;
use serde::{Deserialize, Serialize};
use serenity::async_trait;
use serenity::client::Context;
use serenity::framework::standard::StandardFramework;
use serenity::model::application::command::CommandOptionType;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::prelude::GuildId;
use serenity::model::prelude::Ready;
use serenity::prelude::{EventHandler, GatewayIntents, TypeMapKey};
use serenity::Client;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub discord: DiscordConfig,
    pub dathost: DathostConfig,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DathostConfig {
    pub user: String,
    pub password: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub token: String,
    pub admin_role_id: u64,
    pub application_id: u64,
    pub guild_id: u64,
}

#[derive(PartialEq)]
struct StateContainer {
    state: State,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Veto {
    map: String,
    vetoed_by: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupStep {
    pub match_id: i32,
    pub step_type: StepType,
    pub team_role_id: i64,
    pub map: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupMap {
    pub match_id: i32,
    pub map: String,
    pub picked_by: i64,
    pub start_attack_team_role_id: Option<i64>,
    pub start_defense_team_role_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setup {
    team_one: Option<i64>,
    team_two: Option<i64>,
    team_one_name: String,
    team_two_name: String,
    team_one_conn_str: Option<String>,
    team_two_conn_str: Option<String>,
    maps_remaining: Vec<String>,
    maps: Vec<SetupMap>,
    vetoes: Vec<Veto>,
    series_type: SeriesType,
    match_id: Option<i32>,
    veto_pick_order: Vec<SetupStep>,
    current_step: usize,
    current_phase: State,
    server_id: Option<String>,
}

#[derive(Debug, Copy, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum State {
    MapVeto,
    SidePick,
    ServerPick,
}

struct Handler;

struct Maps;

struct Matches;

struct DBConnectionPool;

impl TypeMapKey for Config {
    type Value = Config;
}

impl TypeMapKey for Maps {
    type Value = Vec<String>;
}

impl TypeMapKey for Setup {
    type Value = Setup;
}

impl TypeMapKey for Matches {
    type Value = Vec<Match>;
}

impl TypeMapKey for DBConnectionPool {
    type Value = PgPool;
}

#[derive(Debug)]
#[repr(u8)]
enum Command {
    AddMatch,
    DeleteMatch,
    Maps,
    Match,
    Matches,
    Schedule,
    Setup,
    SteamId,
}

impl FromStr for Command {
    type Err = Error;

    fn from_str(input: &str) -> Result<Command, Self::Err> {
        match input {
            "addmatch" => Ok(Command::AddMatch),
            "deletematch" => Ok(Command::DeleteMatch),
            "maps" => Ok(Command::Maps),
            "match" => Ok(Command::Match),
            "matches" => Ok(Command::Matches),
            "schedule" => Ok(Command::Schedule),
            "setup" => Ok(Command::Setup),
            "steamid" => Ok(Command::SteamId),
            _ => Err(Error::UnknownCommand(input.to_string())),
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, context: Context, ready: Ready) {
        let data = context.data.read().await;
        let config = data.get::<Config>().unwrap();
        let guild_id = GuildId(config.discord.guild_id);
        let commands = GuildId::set_application_commands(&guild_id, &context.http, |commands| {
            return commands
                .create_application_command(|command| {
                    command
                        .name("maps")
                        .description("Lists the current map pool")
                })
                .create_application_command(|command| {
                    command
                        .name("steamid")
                        .description("Set your SteamID")
                        .create_option(|option| {
                            option
                                .name("steamid")
                                .description("Your steamID, i.e. STEAM_0:1:12345678")
                                .kind(CommandOptionType::String)
                                .required(true)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("match")
                        .description("Show match info")
                        .create_option(|option| {
                            option
                                .name("matchid")
                                .description("Match ID")
                                .kind(CommandOptionType::String)
                                .required(true)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("matches")
                        .description("Show matches")
                        .create_option(|option| {
                            option
                                .name("showcompleted")
                                .description("Shows only completed matches")
                                .kind(CommandOptionType::Boolean)
                                .required(false)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("deletematch")
                        .description("Delete match (admin required)")
                        .create_option(|option| {
                            option
                                .name("matchid")
                                .description("Match ID")
                                .kind(CommandOptionType::Integer)
                                .required(true)
                        })
                })
                .create_application_command(|command| {
                    command.name("setup").description("Setup your next match")
                })
                .create_application_command(|command| {
                    command
                        .name("addmatch")
                        .description("Add match to schedule (admin required)")
                        .create_option(|option| {
                            option
                                .name("teamone")
                                .description("Team 1 (Home)")
                                .kind(CommandOptionType::Role)
                                .required(true)
                        })
                        .create_option(|option| {
                            option
                                .name("teamtwo")
                                .description("Team 2 (Away)")
                                .kind(CommandOptionType::Role)
                                .required(true)
                        })
                        .create_option(|option| {
                            option
                                .name("type")
                                .description("Series Type")
                                .kind(CommandOptionType::String)
                                .required(true)
                                .add_string_choice("Best of 1", "bo1")
                                .add_string_choice("Best of 3", "bo3")
                                .add_string_choice("Best of 5", "bo5")
                        })
                        .create_option(|option| {
                            option
                                .name("note")
                                .description("Note")
                                .kind(CommandOptionType::String)
                                .required(false)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("schedule")
                        .description("Schedule your next match")
                        .create_option(|option| {
                            option
                                .name("date")
                                .description("Date (Month/Day/Year) @ Time <Timezone>")
                                .kind(CommandOptionType::String)
                                .required(true)
                        })
                });
        })
        .await;
        println!("{} is connected!", ready.user.name);
        log::debug!("Added these guild slash commands: {:#?}", commands);
    }

    async fn interaction_create(&self, context: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(interaction) = interaction {
            let command = interaction.data.name.to_lowercase();

            if let Ok(normal_command) = Command::from_str(&command) {
                let response = match normal_command {
                    Command::AddMatch => commands::handle_add_match(&context, &interaction).await,
                    Command::DeleteMatch => {
                        commands::handle_delete_match(&context, &interaction).await
                    }
                    Command::Maps => commands::handle_map_list(&context).await,
                    Command::Match => commands::handle_match(&context, &interaction).await,
                    Command::Matches => commands::handle_matches(&context, &interaction).await,
                    Command::Schedule => commands::handle_schedule(&context, &interaction).await,
                    Command::Setup => commands::handle_setup(&context, &interaction).await,
                    Command::SteamId => commands::handle_steam_id(&context, &interaction).await,
                };

                match response {
                    Ok(Some(response)) => {
                        if let Err(why) =
                            create_int_resp(&context, &interaction, response.into_owned()).await
                        {
                            log::error!("Cannot respond to slash command: {}", why);
                        }
                    }
                    Ok(None) => {}
                    Err(err) => {
                        log::error!("Error handling command: {}", err);
                    }
                }
            }
        }
    }
}

async fn create_int_resp(
    context: &Context,
    inc_command: &ApplicationCommandInteraction,
    content: String,
) -> serenity::Result<()> {
    inc_command
        .create_interaction_response(context, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.ephemeral(true).content(content))
        })
        .await
}

fn config_from_env() -> Result<Config, Error> {
    use dotenvy::var;

    Ok(Config {
        discord: DiscordConfig {
            token: var("DISCORD_TOKEN")?,
            admin_role_id: var("DISCORD_ADMIN_ROLE_ID")?.parse()?,
            application_id: var("DISCORD_APPLICATION_ID")?.parse()?,
            guild_id: var("DISCORD_GUILD_ID")?.parse()?,
        },
        dathost: DathostConfig {
            user: var("DATHOST_USER")?,
            password: var("DATHOST_PASSWORD")?,
        },
    })
}

pub async fn establish_pool() -> Result<PgPool, Error> {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    Ok(PgPoolOptions::new()
        .max_connections(15)
        .connect(&database_url)
        .await?)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let _ = dotenvy::dotenv();
    env_logger::init();

    let config = config_from_env()?;

    let token = &config.discord.token;
    let framework = StandardFramework::new();
    let intents = GatewayIntents::non_privileged() | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(token, intents)
        .event_handler(Handler {})
        .framework(framework)
        .application_id(config.discord.application_id)
        .await
        .expect("Error creating client");
    {
        let mut data = client.data.write().await;
        data.insert::<Config>(config);
        data.insert::<DBConnectionPool>(establish_pool().await?);
    }
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }

    Ok(())
}
