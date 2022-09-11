use crate::dathost_models::DathostServerDuplicateResponse;
use crate::error::Error;
use crate::{Config, DBConnectionPool, DathostConfig, Setup, SetupStep};
use anyhow::{anyhow, bail};
use csgo_matchbot::helpers::create_server_conn_button_row;
use csgo_matchbot::models::StepType::{Pick, Veto};
use csgo_matchbot::models::{Match, MatchSetupStep, MatchState, SeriesType};
use csgo_matchbot::{
    create_match_setup_step, create_series_map, get_fresh_token, get_map_pool,
    get_user_by_discord_id, update_match_state, update_token, DiscordId, SeriesMap,
};
use futures::stream::{self, StreamExt};
use once_cell::sync::Lazy;
use reqwest::{Client, Response, StatusCode};
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::application::interaction::message_component::MessageComponentInteraction;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::channel::Message;
use serenity::model::id::GuildId;
use serenity::model::prelude::interaction::application_command::CommandDataOptionValue;
use serenity::model::prelude::{Member, Role, RoleId, User};
use serenity::prelude::Context;
use serenity::utils::MessageBuilder;
use sqlx::PgPool;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use steamid::SteamId;
use urlencoding::encode;

pub async fn find_user_team_role(
    guild_roles: &[Role],
    user: &User,
    context: &Context,
) -> anyhow::Result<Role> {
    let team_roles = guild_roles
        .iter()
        .filter(|r| r.name.starts_with("Team"))
        .collect::<Vec<_>>();

    for team_role in team_roles {
        if let Ok(has_role) = user
            .has_role(context, team_role.guild_id, team_role.id)
            .await
        {
            if !has_role {
                continue;
            }
            return Ok(team_role.clone());
        }
    }

    bail!("User does not have a team role");
}

pub async fn user_team_author(
    context: &Context,
    setup: &Setup,
    component_interation: &Arc<MessageComponentInteraction>,
) -> anyhow::Result<u64> {
    let team_one = setup.team_one.unwrap() as u64;
    let team_two = setup.team_two.unwrap() as u64;
    let guild_id = component_interation.guild_id.unwrap();

    let is_team_one = component_interation
        .user
        .has_role(&context, guild_id, team_one)
        .await?;
    if is_team_one {
        return Ok(team_one);
    }

    let is_team_two = component_interation
        .user
        .has_role(&context, guild_id, team_two)
        .await?;
    if is_team_two {
        return Ok(team_two);
    }

    bail!("You are not part of either team currently running `/setup`");
}

pub async fn admin_check(
    context: &Context,
    interaction: &ApplicationCommandInteraction,
) -> anyhow::Result<()> {
    let data = context.data.read().await;
    let config = data
        .get::<Config>()
        .ok_or_else(|| anyhow!("config missing from context"))?;
    let guild_id = interaction
        .guild_id
        .ok_or_else(|| anyhow!("interaction has no guild"))?;
    let role_id = RoleId::from(config.discord.admin_role_id);

    let user_has_role = interaction
        .user
        .has_role(context, guild_id, role_id)
        .await
        .unwrap_or(false);

    if user_has_role {
        return Ok(());
    }

    let role_name = context
        .cache
        .role(guild_id, role_id)
        .map(|role| role.name)
        .ok_or_else(|| anyhow!("admin role not found"))?;
    bail!(MessageBuilder::new()
        .mention(&interaction.user)
        .push(" this command requires the '")
        .push(role_name)
        .push("' role.")
        .build());
}

pub async fn get_maps(context: &Context) -> anyhow::Result<Vec<String>> {
    Ok(get_map_pool(&get_pool(context).await?)
        .await?
        .into_iter()
        .map(|m| m.name)
        .collect())
}

pub async fn finish_setup(context: &Context, setup: &Setup) -> anyhow::Result<()> {
    let pool = get_pool(context).await?;
    let mut transaction = pool.begin().await?;

    let match_id = setup.match_id.unwrap();
    for setup_step in &setup.veto_pick_order {
        let new_step = MatchSetupStep {
            id: None,
            match_id,
            step_type: setup_step.step_type,
            team_role_id: setup_step.team_role_id,
            map: setup_step.map.clone(),
        };

        create_match_setup_step(&mut transaction, new_step).await?;
    }

    for setup_map in &setup.maps {
        let new_series_map = SeriesMap {
            id: None,
            match_id,
            map: setup_map.map.clone(),
            picked_by_role_id: setup_map.picked_by,
            start_attack_team_role_id: setup_map.start_attack_team_role_id,
            start_defense_team_role_id: setup_map.start_defense_team_role_id,
        };

        create_series_map(&mut transaction, new_series_map).await?;
    }

    update_match_state(&mut transaction, match_id, MatchState::Completed).await?;
    Ok(transaction.commit().await?)
}

pub fn print_veto_info(setup_info: &Vec<MatchSetupStep>, match_: &Match) -> anyhow::Result<String> {
    if setup_info.is_empty() {
        bail!("_This match has no veto info yet_");
    }

    let veto: String = setup_info
        .clone()
        .iter()
        .filter(|veto| {
            veto.map
                .as_deref()
                .map(|map| !map.is_empty())
                .unwrap_or(false)
        })
        .map(|veto| {
            let team_name = if match_.team_one_role_id == veto.team_role_id {
                &match_.team_one_name
            } else {
                &match_.team_two_name
            };

            let map = veto
                .map
                .as_ref()
                .ok_or_else::<Infallible, _>(|| unreachable!())?
                .to_lowercase();

            Ok(if veto.step_type == Veto {
                format!("- {} banned {}\n", team_name, map)
            } else {
                format!("+ {} picked {}\n", team_name, map)
            })
        })
        .collect::<anyhow::Result<String>>()?;

    Ok(format!("```diff\n{veto}\n```"))
}

pub fn print_match_info(match_: &Match, show_id: bool) -> String {
    let scheduled = match_
        .scheduled_time_str
        .as_deref()
        .map(|time| format!(" > Scheduled: `{}`", time));
    let scheduled = scheduled.as_deref().unwrap_or("");
    let note = match_.note.as_ref().map(|note| format!(" `{}`", note));
    let note = note.as_deref().unwrap_or("");
    let id = show_id
        .then_some(match_.id)
        .flatten()
        .map(|id| format!("    _Match ID:_ `{}\n`", id));
    let id = id.as_deref().unwrap_or("");

    format!(
        "- {} vs {}{}{}{}",
        match_.team_one_name, match_.team_two_name, scheduled, note, id
    )
}

pub fn eos_printout(setup: &Setup) -> String {
    let maps = setup.maps.iter().enumerate().map(|(index, map)| {
        format!(
            "**{}. {}** - picked by: <@&{}>\n    _CT start:_ <@&{}>\n    _T start:_ <@&{}>\n",
            index + 1,
            map.map.to_lowercase(),
            &map.picked_by,
            map.start_defense_team_role_id.unwrap(),
            map.start_attack_team_role_id.unwrap()
        )
    });

    Some(String::from("\n\nSetup is completed. GLHF!\n"))
        .into_iter()
        .chain(maps)
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn interaction_response(
    context: &Context,
    interaction: &Arc<MessageComponentInteraction>,
    content: &str,
) -> serenity::Result<()> {
    interaction
        .create_interaction_response(&context, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|response_data| {
                    response_data.ephemeral(true).content(content)
                })
        })
        .await
}

pub fn handle_bo1_setup(setup: &Setup) -> (Vec<SetupStep>, String) {
    let match_id = setup.match_id.unwrap();
    let team_one = setup.team_one.unwrap();
    let team_two = setup.team_two.unwrap();

    (
        vec![
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_one,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_one,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_one,
                map: None,
            },
        ],
        format!(
            "Best of 1 option selected. Starting map veto. <@&{}> bans first.\n",
            &team_two
        ),
    )
}

pub fn handle_bo3_setup(setup: &Setup) -> (Vec<SetupStep>, String) {
    let match_id = setup.match_id.unwrap();
    let team_one = setup.team_one.unwrap();
    let team_two = setup.team_two.unwrap();

    (
        vec![
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_one,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_one,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_one,
                map: None,
            },
        ],
        format!(
            "Best of 3 option selected. Starting map veto. <@&{}> bans first.\n",
            &setup.team_one.unwrap()
        ),
    )
}

pub fn handle_bo5_setup(setup: &Setup) -> (Vec<SetupStep>, String) {
    let match_id = setup.match_id.unwrap();
    let team_one = setup.team_one.unwrap();
    let team_two = setup.team_two.unwrap();

    (
        vec![
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_one,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Veto,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_one,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_one,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_two,
                map: None,
            },
            SetupStep {
                match_id,
                step_type: Pick,
                team_role_id: team_one,
                map: None,
            },
        ],
        format!(
            "Best of 5 option selected. Starting map veto. <@&{}> bans first.\n",
            &team_one
        ),
    )
}

pub async fn get_pool(context: &Context) -> Result<PgPool, Error> {
    let data = context.data.read().await;
    let pool = data
        .get::<DBConnectionPool>()
        .ok_or(Error::MissingFromContext("database pool"))?;
    Ok(PgPool::clone(pool))
}

pub async fn duplicate_server(
    server_id: impl AsRef<str>,
    auth: &DathostConfig,
) -> anyhow::Result<DathostServerDuplicateResponse> {
    static CLIENT: Lazy<Client> = Lazy::new(Client::new);

    let server_id = encode(server_id.as_ref());
    Ok(CLIENT
        .post(format!(
            "https://dathost.net/api/0.1/game-servers/{server_id}/duplicate"
        ))
        .basic_auth(&auth.user, Some(&auth.password))
        .send()
        .await?
        .json()
        .await?)
}

pub async fn update_server(
    server_id: impl AsRef<str>,
    name: impl AsRef<str>,
    token: impl AsRef<str>,
    auth: &DathostConfig,
) -> anyhow::Result<StatusCode> {
    static CLIENT: Lazy<Client> = Lazy::new(Client::new);

    let server_id = encode(server_id.as_ref());
    Ok(CLIENT
        .put(format!(
            "https://dathost.net/api/0.1/game-servers/{server_id}"
        ))
        .basic_auth(&auth.user, Some(&auth.password))
        .form(&[
            ("name", name.as_ref()),
            (
                "csgo_settings.steam_game_server_login_token",
                token.as_ref(),
            ),
        ])
        .send()
        .await?
        .status())
}

pub async fn start_server(
    context: &Context,
    guild_id: GuildId,
    setup: &mut Setup,
) -> anyhow::Result<DathostServerDuplicateResponse> {
    log::info!("{:#?}", setup);

    let data = context.data.write().await;
    let config: &Config = data.get::<Config>().unwrap();

    let dathost_config = &config.dathost;
    let pool = get_pool(context).await?;

    log::info!("duplicating server");
    let server = duplicate_server(setup.server_id.as_ref().unwrap(), dathost_config).await?;
    let server_id = &server.id;

    let mut game_server_login_token = get_fresh_token(&pool)
        .await?
        .ok_or_else(|| anyhow!("no game server login token found"))?;

    log::info!("updating game server with name and gslt");
    let match_id = setup.match_id.unwrap();
    let status = update_server(
        server_id,
        format!("match-server-{match_id}"),
        &game_server_login_token.token,
        dathost_config,
    )
    .await?;
    if status.is_success() {
        game_server_login_token.in_use = true;
        update_token(&pool, &game_server_login_token).await?;
    }

    let members: Vec<Member> = context
        .http
        .get_guild_members(*guild_id.as_u64(), None, None)
        .await?;

    let team_one = setup.team_one.unwrap() as u64;
    let team_two = setup.team_two.unwrap() as u64;
    let (team_one_users, team_two_users) = stream::iter(members)
        .map(|member| member.user)
        .then(|user| async move {
            if user.has_role(&context, guild_id, team_one).await? {
                return Ok(Some((Some(user), None)));
            }

            if user.has_role(&context, guild_id, team_two).await? {
                return Ok(Some((None, Some(user))));
            }

            Result::<_, anyhow::Error>::Ok(None)
        })
        .filter_map(|user| async move { user.ok() })
        .filter_map(|user| async move { user })
        .collect::<(Vec<_>, Vec<_>)>()
        .await;
    let team_one_users = team_one_users.iter().flatten().collect::<Vec<_>>();
    let team_two_users = team_two_users.iter().flatten().collect::<Vec<_>>();
    log::info!("1: {:#?}", team_one_users);
    log::info!("2: {:#?}", team_two_users);

    setup.team_one_conn_str = Some(map_steamid_strings(context, team_one_users).await?);
    setup.team_two_conn_str = Some(map_steamid_strings(context, team_two_users).await?);
    log::info!(
        "starting match\nteam1 '{}'\nteam2: '{}'",
        setup.team_one_conn_str.as_deref().unwrap(),
        setup.team_two_conn_str.as_deref().unwrap()
    );

    let response = match setup.series_type {
        SeriesType::Bo1 => start_match(server_id, setup, dathost_config).await?,
        SeriesType::Bo3 => start_series_match(server_id, setup, dathost_config).await?,
        SeriesType::Bo5 => start_series_match(server_id, setup, dathost_config).await?,
    };

    log::info!("{:#?}", response.text().await?);
    Ok(server)
}

pub async fn start_match(
    server_id: impl AsRef<str>,
    setup: &Setup,
    dathost_config: &DathostConfig,
) -> anyhow::Result<Response> {
    static CLIENT: Lazy<Client> = Lazy::new(Client::new);

    let (mut team_ct, mut team_ct_name, mut team_t, mut team_t_name) = (
        setup.team_one_conn_str.as_deref().unwrap(),
        setup.team_one_name.as_str(),
        setup.team_two_conn_str.as_deref().unwrap(),
        setup.team_two_name.as_str(),
    );

    if setup.maps[0].start_defense_team_role_id != setup.team_one {
        core::mem::swap(&mut team_ct, &mut team_t);
        core::mem::swap(&mut team_ct_name, &mut team_t_name);
    };

    log::info!("starting match request...");
    Ok(CLIENT
        .post("https://dathost.net/api/0.1/matches")
        .form(&[
            ("game_server_id", server_id.as_ref()),
            ("map", setup.maps[0].map.as_str()),
            ("team1_name", team_t_name),
            ("team2_name", team_ct_name),
            ("team1_steam_ids", team_t),
            ("team2_steam_ids", team_ct),
            ("enable_pause", "true"),
            ("enable_tech_pause", "true"),
        ])
        .basic_auth(&dathost_config.user, Some(&dathost_config.password))
        .send()
        .await?)
}

pub async fn start_series_match(
    server_id: impl AsRef<str>,
    setup: &mut Setup,
    dathost_config: &DathostConfig,
) -> anyhow::Result<Response> {
    static CLIENT: Lazy<Client> = Lazy::new(Client::new);

    let team_one = setup.team_one_conn_str.as_deref().unwrap();
    let team_one_name = setup.team_one_name.as_str();
    let team_two = setup.team_two_conn_str.as_deref().unwrap();
    let team_two_name = setup.team_two_name.as_str();

    let which_team = |role_id| {
        if role_id == setup.team_one {
            "team1"
        } else {
            "team2"
        }
    };

    let mut params = HashMap::from([
        ("game_server_id", server_id.as_ref()),
        ("enable_pause", "true"),
        ("enable_tech_pause", "true"),
        ("team1_name", team_one_name),
        ("team2_name", team_two_name),
        ("team1_steam_ids", team_one),
        ("team2_steam_ids", team_two),
        ("number_of_maps", "3"),
        ("map1", setup.maps[0].map.as_str()),
        (
            "map1_start_ct",
            which_team(setup.maps[0].start_defense_team_role_id),
        ),
        ("map2", setup.maps[1].map.as_str()),
        (
            "map2_start_ct",
            which_team(setup.maps[1].start_defense_team_role_id),
        ),
        ("map3", setup.maps[2].map.as_str()),
        (
            "map3_start_ct",
            which_team(setup.maps[2].start_defense_team_role_id),
        ),
    ]);

    if setup.series_type == SeriesType::Bo5 {
        params.insert("map4", setup.maps[3].map.as_str());
        params.insert(
            "map4_start_ct",
            which_team(setup.maps[3].start_defense_team_role_id),
        );
        params.insert("map5", setup.maps[4].map.as_str());
        params.insert(
            "map5_start_ct",
            which_team(setup.maps[4].start_defense_team_role_id),
        );
        params.insert("number_of_maps", "5");
    }

    log::info!("{:#?}", params);

    Ok(CLIENT
        .post("https://dathost.net/api/0.1/match-series")
        .form(&params)
        .basic_auth(&dathost_config.user, Some(&dathost_config.password))
        .send()
        .await?)
}

pub async fn map_steamid_strings(context: &Context, users: Vec<&User>) -> anyhow::Result<String> {
    let pool = get_pool(context).await?;
    Ok(stream::iter(users)
        .map(|user| *user.id.as_u64())
        .map(DiscordId::from)
        .then(|discord_id| get_user_by_discord_id(&pool, discord_id))
        .filter_map(|user| async move {
            match &user {
                Ok(Some(_)) => {}
                Ok(None) => log::warn!("User not found in database"),
                Err(e) => log::error!("Error getting user: {}", e),
            }
            user.ok().flatten()
        })
        .map(|user| SteamId::from(user.steam_id).steam2id())
        .collect::<Vec<_>>()
        .await
        .join(","))
}

pub async fn create_tiny_url(url: impl AsRef<str>) -> anyhow::Result<String> {
    static CLIENT: Lazy<Client> = Lazy::new(Client::new);
    Ok(CLIENT
        .get(format!(
            "https://tinyurl.com/api-create.php?url={}",
            encode(url.as_ref())
        ))
        .send()
        .await?
        .text_with_charset("utf-8")
        .await?)
}

pub async fn connect_message(
    context: &Context,
    message: &Message,
    server: DathostServerDuplicateResponse,
    setup: &Setup,
) -> anyhow::Result<()> {
    let game_url = format!("{}:{}", server.ip, server.ports.game);
    let gotv_url = format!("{}:{}", server.ip, server.ports.gotv);
    let url_link = format!("steam://connect/{}", &game_url);
    let gotv_link = format!("steam://connect/{}", &gotv_url);

    let t_game_url = create_tiny_url(url_link).await?;
    let t_gotv_url = create_tiny_url(gotv_link).await?;

    let mut message = message
        .channel_id
        .send_message(context, |message| {
            message
                .content(eos_printout(setup))
                .components(|components| {
                    components.add_action_row(create_server_conn_button_row(
                        &t_game_url,
                        &t_gotv_url,
                        true,
                    ))
                })
        })
        .await?;

    let mut interaction_collector = message
        .await_component_interactions(context)
        .timeout(Duration::from_secs(60 * 5))
        .build();

    while let Some(interaction) = interaction_collector.next().await {
        interaction
            .create_interaction_response(context, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|response_data| {
                        response_data.ephemeral(true).content(format!(
                            "Console: ||`connect {}`||\nGOTV: ||`connect {}`||",
                            &game_url, &gotv_url
                        ))
                    })
            })
            .await?;
    }

    message
        .edit(context, |message| {
            message
                .content(eos_printout(setup))
                .components(|components| {
                    components.add_action_row(create_server_conn_button_row(
                        &t_game_url,
                        &t_gotv_url,
                        false,
                    ))
                })
        })
        .await?;
    Ok(())
}

pub fn get_option(
    interaction: &ApplicationCommandInteraction,
    index: usize,
) -> anyhow::Result<&CommandDataOptionValue> {
    interaction
        .data
        .options
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("No option at index {}", index))?
        .resolved
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No resolved value at index {}", index))
}

pub fn get_option_as_bool(
    interaction: &ApplicationCommandInteraction,
    index: usize,
) -> anyhow::Result<bool> {
    match get_option(interaction, index)? {
        CommandDataOptionValue::Boolean(value) => Ok(*value),
        _ => Err(anyhow::anyhow!("Option at index {} is not a bool", index)),
    }
}

pub fn get_option_as_string(
    interaction: &ApplicationCommandInteraction,
    index: usize,
) -> anyhow::Result<&String> {
    match get_option(interaction, index)? {
        CommandDataOptionValue::String(value) => Ok(value),
        _ => Err(anyhow::anyhow!("Option at index {} is not a string", index)),
    }
}

pub fn get_option_as_integer(
    interaction: &ApplicationCommandInteraction,
    index: usize,
) -> anyhow::Result<i64> {
    match get_option(interaction, index)? {
        CommandDataOptionValue::Integer(value) => Ok(*value),
        _ => Err(anyhow::anyhow!(
            "Option at index {} is not a integer",
            index
        )),
    }
}

pub fn get_option_as_role(
    interaction: &ApplicationCommandInteraction,
    index: usize,
) -> anyhow::Result<&Role> {
    match get_option(interaction, index)? {
        CommandDataOptionValue::Role(value) => Ok(value),
        _ => Err(anyhow::anyhow!("Option at index {} is not a role", index)),
    }
}
