use std::borrow::Cow;
use std::convert::Infallible;
use std::str::FromStr;

use anyhow::{anyhow, bail};
use csgo_matchbot::get_match_servers;
use csgo_matchbot::helpers::create_map_action_row;
use csgo_matchbot::helpers::create_server_action_row;
use csgo_matchbot::helpers::create_sidepick_action_row;
use csgo_matchbot::models::{Match, MatchState, SeriesType};
use csgo_matchbot::DiscordId;
use csgo_matchbot::MatchSetupStep;
use csgo_matchbot::StepType;
use csgo_matchbot::{
    create_match, create_user, delete_match, get_match, get_match_setup_steps, get_matches,
    get_next_team_match, update_match_schedule,
};
use futures::StreamExt;
use serenity::client::Context;
use serenity::model::application::interaction::application_command::ApplicationCommandInteraction;
use serenity::model::prelude::interaction::InteractionResponseType;
use serenity::utils::MessageBuilder;
use steamid::AccountType;
use steamid::Instance;
use steamid::SteamId;
use time::OffsetDateTime;

use crate::utils::{
    admin_check, connect_message, find_user_team_role, finish_setup, get_maps, get_option_as_bool,
    get_option_as_integer, get_option_as_role, get_option_as_string, get_pool, handle_bo1_setup,
    handle_bo3_setup, handle_bo5_setup, interaction_response, print_match_info, print_veto_info,
    start_server, user_team_author,
};
use crate::Setup;
use crate::SetupMap;
use crate::State;

type CommandResponse = Result<Option<Cow<'static, str>>, anyhow::Error>;

pub async fn handle_setup(
    context: &Context,
    interaction: &ApplicationCommandInteraction,
) -> CommandResponse {
    let pool = get_pool(context).await?;

    let guild_roles = context
        .http
        .get_guild_roles(*interaction.guild_id.unwrap().as_u64())
        .await?;

    let team_role = if let Ok(role_id) =
        find_user_team_role(&guild_roles, &interaction.user, context).await
    {
        role_id
    } else {
        interaction.create_interaction_response(&context, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| {
                    message
                        .ephemeral(true)
                        .content("You are not part of any team. Verify you have a role starting with `Team`")
                })
        })
        .await?;

        return Ok(None);
    };

    let current_match =
        if let Some(next_match) = get_next_team_match(&pool, team_role.id.0 as i64).await? {
            next_match
        } else {
            interaction
                .create_interaction_response(&context, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| {
                            message
                                .ephemeral(true)
                                .content("Your team does not have any scheduled matches")
                        })
                })
                .await?;

            return Ok(None);
        };

    interaction
        .create_interaction_response(&context, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| message.content("Starting setup..."))
        })
        .await?;

    let maps = get_maps(context).await?;
    let mut setup = Setup {
        maps_remaining: maps,
        maps: vec![],
        vetoes: vec![],
        series_type: current_match.series_type,
        team_one_name: current_match.team_one_name,
        team_two_name: current_match.team_two_name,
        team_one: Some(current_match.team_one_role_id),
        team_two: Some(current_match.team_two_role_id),
        match_id: current_match.id,
        veto_pick_order: vec![],
        current_step: 0,
        current_phase: State::ServerPick,
        server_id: None,
        team_two_conn_str: None,
        team_one_conn_str: None,
    };

    let (veto_pick_order, init_veto_msg) = match current_match.series_type {
        SeriesType::Bo1 => handle_bo1_setup(&setup),
        SeriesType::Bo3 => handle_bo3_setup(&setup),
        SeriesType::Bo5 => handle_bo5_setup(&setup),
    };
    setup.veto_pick_order = veto_pick_order;

    let match_servers = get_match_servers(&pool).await?;
    let message = interaction
        .channel_id
        .send_message(&context, |m| {
            m.content(format!("<@&{}> selects server.", setup.team_two.unwrap()))
                .components(|c| c.add_action_row(create_server_action_row(&match_servers)))
        })
        .await?;

    // Wait for the user to make a selection
    let mut component_interactions = message.await_component_interactions(context).build();
    while let Some(component_interaction) = component_interactions.next().await {
        let role_id =
            if let Ok(role_id) = user_team_author(context, &setup, &component_interaction).await {
                role_id
            } else {
                interaction_response(
                    context,
                    &component_interaction,
                    "You are not part of either team currently setting up a match",
                )
                .await?;
                continue;
            };

        match setup.current_phase {
            State::ServerPick => {
                if role_id != setup.team_two.unwrap() as u64 {
                    interaction_response(
                        context,
                        &component_interaction,
                        "It is not your team's turn to pick or ban a server",
                    )
                    .await?;
                    continue;
                }

                let server_id = component_interaction.data.values[0].as_str();
                setup.server_id = Some(server_id.to_string());

                component_interaction
                    .create_interaction_response(&context, |response| {
                        response
                            .kind(InteractionResponseType::UpdateMessage)
                            .interaction_response_data(|message| {
                                message.content(&init_veto_msg).components(|components| {
                                    components.add_action_row(create_map_action_row(
                                        &setup.maps_remaining[..],
                                        setup.veto_pick_order[0].step_type,
                                    ))
                                })
                            })
                    })
                    .await?;

                setup.current_phase = State::MapVeto;
            }
            State::MapVeto => {
                let current_step = &mut setup.veto_pick_order[setup.current_step];
                let current_picker = current_step.team_role_id;
                if role_id != current_picker as u64 {
                    interaction_response(
                        context,
                        &component_interaction,
                        "It is not your team's turn to pick or ban a map",
                    )
                    .await?;
                    continue;
                }

                let map_selected = component_interaction.data.values[0].as_str();
                current_step.map = Some(map_selected.to_string());

                if current_step.step_type == StepType::Pick {
                    setup.maps.push(SetupMap {
                        map: map_selected.to_string(),
                        picked_by: role_id as i64,
                        match_id: 0,
                        start_attack_team_role_id: None,
                        start_defense_team_role_id: None,
                    });
                }

                if setup.current_step + 1 == setup.veto_pick_order.len() {
                    let first_map = &setup.maps[0];
                    let next_picker = if first_map.picked_by == setup.team_one.unwrap() {
                        setup.team_two
                    } else {
                        setup.team_one
                    }
                    .unwrap();

                    component_interaction.create_interaction_response(context, |response| {
                        response
                            .kind(InteractionResponseType::UpdateMessage)
                            .interaction_response_data(|response_data| {
                                response_data
                                    .content(format!(
                                        "Map veto completed.\nIt is <@&{}> turn to pick starting side for `{}`",
                                        next_picker,
                                        first_map.map
                                    ))
                                    .components(|components| components.add_action_row(create_sidepick_action_row()))
                            })
                    }).await?;

                    setup.current_step = 0;
                    setup.current_phase = State::SidePick;
                    continue;
                }

                log::info!("Map selected: {}", map_selected);
                setup.maps_remaining.retain(|map| map != map_selected);

                let next_step = &setup.veto_pick_order[setup.current_step + 1];
                let next_step_type = next_step.step_type;
                let next_role_id = next_step.team_role_id;

                let setup_info = setup
                    .veto_pick_order
                    .iter()
                    .map(|setup_step| MatchSetupStep {
                        id: None,
                        match_id: 0,
                        step_type: setup_step.step_type,
                        team_role_id: setup_step.team_role_id,
                        map: setup_step.map.clone(),
                    })
                    .collect::<Vec<_>>();
                let match_ = Match {
                    id: None,
                    team_one_role_id: setup.team_one.unwrap(),
                    team_one_name: setup.team_one_name.clone(),
                    team_two_role_id: setup.team_two.unwrap(),
                    team_two_name: setup.team_two_name.clone(),
                    note: None,
                    date_added: OffsetDateTime::now_utc(),
                    match_state: MatchState::Entered,
                    scheduled_time_str: None,
                    series_type: SeriesType::Bo1,
                };

                let veto_info = print_veto_info(&setup_info, &match_)?;
                component_interaction
                    .create_interaction_response(context, |response| {
                        response
                            .kind(InteractionResponseType::UpdateMessage)
                            .interaction_response_data(|response_data| {
                                response_data
                                    .content(format!(
                                        "{}\nIt is <@&{}> turn to {}",
                                        veto_info, next_role_id, next_step_type
                                    ))
                                    .components(|components| {
                                        components.add_action_row(create_map_action_row(
                                            &setup.maps_remaining,
                                            next_step_type,
                                        ))
                                    })
                            })
                    })
                    .await?;

                setup.current_step += 1;
            }
            State::SidePick => {
                let side_picked = &component_interaction.data.values[0];
                let map_picked_by = setup.maps[setup.current_step].picked_by;

                if role_id == map_picked_by as u64 {
                    interaction_response(
                        context,
                        &component_interaction,
                        "It is not your team's turn to pick sides",
                    )
                    .await?;
                    continue;
                }

                if setup.current_step + 1 == setup.maps.len() {
                    let new_message = message
                        .channel_id
                        .send_message(context, |message| {
                            message.content("Match setup completed, starting server...")
                        })
                        .await?;
                    message.delete(context).await?;

                    match start_server(context, message.guild_id.unwrap(), &mut setup).await {
                        Ok(response) => {
                            finish_setup(context, &setup).await?;
                            connect_message(context, &new_message, response, &setup).await?;

                            return Ok(None);
                        }
                        Err(err) => {
                            log::error!("{:#?}", err);
                        }
                    }
                }

                let next_step = &setup.maps[setup.current_step];
                let next_map_picker = next_step.picked_by;
                let next_map = &next_step.map;

                let next_picker = if next_map_picker == map_picked_by {
                    role_id
                } else {
                    map_picked_by as u64
                };

                component_interaction
                    .create_interaction_response(context, |response| {
                        response
                            .kind(InteractionResponseType::UpdateMessage)
                            .interaction_response_data(|response_data| {
                                response_data.content(format!(
                                    "It is <@&{}> turn to pick starting side on {}",
                                    next_picker, next_map
                                ))
                            })
                    })
                    .await?;

                let current_step = &mut setup.maps[setup.current_step];
                if side_picked == "ct" {
                    current_step.start_defense_team_role_id = Some(role_id as i64);
                    current_step.start_attack_team_role_id = Some(map_picked_by);
                } else {
                    current_step.start_defense_team_role_id = Some(map_picked_by);
                    current_step.start_attack_team_role_id = Some(role_id as i64);
                }

                setup.current_step += 1;
            }
        }
    }
    Ok(None)
}

pub async fn handle_map_list(context: &Context) -> CommandResponse {
    let map_str = get_maps(context)
        .await?
        .into_iter()
        .map(|map| format!("- `{}`", map))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Some(
        MessageBuilder::new()
            .push_line("Current map pool:")
            .push(map_str)
            .build()
            .into(),
    ))
}

pub async fn handle_schedule(
    context: &Context,
    interaction: &ApplicationCommandInteraction,
) -> CommandResponse {
    let timestamp = get_option_as_string(interaction, 0)?;

    let guild_id = interaction
        .guild_id
        .ok_or_else(|| anyhow!("Expected guild id"))?;
    let roles = context.http.get_guild_roles(*guild_id.as_u64()).await?;
    let team_roles = roles
        .iter()
        .filter(|role| role.name.starts_with("Team "))
        .collect::<Vec<_>>();

    for team_role in team_roles {
        let has_role = interaction
            .user
            .has_role(context, team_role.guild_id, team_role.id)
            .await?;

        if has_role {
            let pool = get_pool(context).await?;

            let next_match = get_next_team_match(&pool, team_role.id.0 as i64)
                .await?
                .ok_or_else(|| anyhow!("Your team does not have any scheduled matches"))?;

            let match_id = next_match
                .id
                .ok_or_else::<Infallible, _>(|| unreachable!())?;
            update_match_schedule(&pool, match_id, &timestamp).await?;

            return Ok(Some(
                format!(
                    "Your next match (<@&{}> vs <@&{}>) is scheduled for `{}`",
                    next_match.team_one_role_id, next_match.team_two_role_id, &timestamp
                )
                .into(),
            ));
        }
    }

    bail!("You are not part of any team. Verify you have a role starting with `Team`");
}

pub async fn handle_match(
    context: &Context,
    interaction: &ApplicationCommandInteraction,
) -> CommandResponse {
    let match_id = i32::try_from(get_option_as_integer(interaction, 0)?)?;

    let pool = get_pool(context).await?;
    let match_ = get_match(&pool, match_id)
        .await?
        .ok_or_else(|| anyhow!("Match not found"))?;
    let steps = get_match_setup_steps(&pool, match_id).await?;

    Ok(Some(
        format!(
            "{}{}",
            print_match_info(&match_, false),
            print_veto_info(&steps, &match_)?
        )
        .into(),
    ))
}

pub async fn handle_matches(
    context: &Context,
    interaction: &ApplicationCommandInteraction,
) -> CommandResponse {
    let match_state = if get_option_as_bool(interaction, 0)? {
        MatchState::Completed
    } else {
        MatchState::Scheduled
    };

    let matches = get_matches(&get_pool(context).await?, 20, match_state).await?;

    if matches.is_empty() {
        bail!("No matches found");
    }

    Ok(Some(
        matches
            .iter()
            .map(|match_| print_match_info(match_, true))
            .collect::<String>()
            .into(),
    ))
}

pub async fn handle_add_match(
    context: &Context,
    interaction: &ApplicationCommandInteraction,
) -> CommandResponse {
    admin_check(context, interaction).await?;

    let team_one = get_option_as_role(interaction, 0)?;
    let team_two = get_option_as_role(interaction, 1)?;
    let series_type = SeriesType::from_str(get_option_as_string(interaction, 2)?)
        .map_err(|_| anyhow!("Invalid series type"))?;
    let note = get_option_as_string(interaction, 3).ok();

    let new_match = Match {
        id: None,
        team_one_role_id: *team_one.id.as_u64() as i64,
        team_one_name: team_one.name.clone(),
        team_two_role_id: *team_two.id.as_u64() as i64,
        team_two_name: team_two.name.clone(),
        note: note.cloned(),
        series_type,
        date_added: OffsetDateTime::now_utc(),
        match_state: MatchState::Entered,
        scheduled_time_str: None,
    };

    create_match(&get_pool(context).await?, new_match).await?;
    Ok(Some("Successfully added new match".into()))
}

pub async fn handle_delete_match(
    context: &Context,
    interaction: &ApplicationCommandInteraction,
) -> CommandResponse {
    admin_check(context, interaction).await?;

    let match_id = get_option_as_integer(interaction, 0).and_then(|match_id| {
        i32::try_from(match_id).map_err(|_| anyhow!("Invalid value: match id"))
    })?;

    Ok(Some(
        if delete_match(&get_pool(context).await?, match_id).await? {
            "Successfully deleted match".into()
        } else {
            "Unable to find match".into()
        },
    ))
}

#[allow(clippy::similar_names)]
pub async fn handle_steam_id(
    context: &Context,
    interaction: &ApplicationCommandInteraction,
) -> CommandResponse {
    let steam2id = get_option_as_string(interaction, 0)?;
    let steamid = SteamId::parse_steam2id(steam2id, AccountType::Individual, Instance::Desktop)?;

    create_user(
        &get_pool(context).await?,
        DiscordId::from(interaction.user.id.0),
        steamid.into(),
    )
    .await?;

    Ok(Some(
        MessageBuilder::new()
            .push("Updated steamid for ")
            .mention(&interaction.user)
            .push(" to `")
            .push(steam2id)
            .push("`\n")
            .push_line("Your steam community profile (please double check this is correct):")
            .push_line(format!(
                "https://steamcommunity.com/profiles/{}",
                u64::from(steamid)
            ))
            .build()
            .into(),
    ))
}
