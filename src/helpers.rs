use serenity::builder::{CreateActionRow, CreateButton, CreateSelectMenu, CreateSelectMenuOption};
use serenity::model::application::component::ButtonStyle;
use serenity::model::prelude::*;

use crate::{MatchServer, StepType};

#[must_use]
pub fn create_button(
    label: &str,
    style: ButtonStyle,
    emoji: &str,
    url: Option<&str>,
) -> CreateButton {
    let mut button = CreateButton::default();
    button.label(label);
    button.style(style);
    button.emoji(ReactionType::Unicode(emoji.to_string()));
    button.custom_id(label.to_lowercase());
    if let Some(url) = url {
        button.url(url);
    }
    button
}

#[must_use]
pub fn create_server_conn_button_row(
    url: &str,
    gotv_url: &str,
    show_cmds: bool,
) -> CreateActionRow {
    let mut action_row = CreateActionRow::default();
    action_row.add_button(create_button("Connect", ButtonStyle::Link, "🛰", Some(url)));
    if show_cmds {
        action_row.add_button(create_button(
            "Console Cmds",
            ButtonStyle::Secondary,
            "🧾",
            None,
        ));
    }
    action_row.add_button(create_button(
        "GOTV",
        ButtonStyle::Link,
        "📺",
        Some(gotv_url),
    ));
    action_row
}

#[must_use]
pub fn create_action_row(
    custom_id: &str,
    placeholder: &str,
    values: &[(&str, &str)],
) -> CreateActionRow {
    let options = values
        .iter()
        .map(|(label, value)| CreateSelectMenuOption::new(label, value.to_lowercase()))
        .collect();

    let mut menu = CreateSelectMenu::default();
    menu.custom_id(custom_id);
    menu.placeholder(placeholder);
    menu.options(move |f| f.set_options(options));

    let mut ar = CreateActionRow::default();
    ar.add_select_menu(menu);
    ar
}

#[must_use]
pub fn create_map_action_row(map_list: &[String], step_type: StepType) -> CreateActionRow {
    let values: Vec<_> = map_list
        .iter()
        .map(|map_name| (map_name.as_str(), map_name.as_str()))
        .collect();

    create_action_row(
        "map_select",
        &format!("Select map to {}", step_type),
        &values,
    )
}

#[must_use]
pub fn create_server_action_row(server_list: &[MatchServer]) -> CreateActionRow {
    let values: Vec<_> = server_list
        .iter()
        .map(|server| (server.region_label.as_str(), server.server_id.as_str()))
        .collect();

    create_action_row("server_select", "Select server", &values)
}

#[must_use]
pub fn create_sidepick_action_row() -> CreateActionRow {
    create_action_row(
        "side_pick",
        "Select starting side",
        &[("CT", "ct"), ("T", "t")],
    )
}
