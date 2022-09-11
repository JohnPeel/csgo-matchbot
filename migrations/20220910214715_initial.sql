CREATE TABLE users (
    id          SERIAL  PRIMARY KEY,
    discord_id  BIGINT  NOT NULL,
    steam_id    BIGINT  NOT NULL
);

CREATE TABLE matches (
    id                 SERIAL       PRIMARY KEY,
    team_one_role_id   BIGINT       NOT NULL,
    team_one_name      VARCHAR(100) NOT NULL,
    team_two_role_id   BIGINT       NOT NULL,
    team_two_name      VARCHAR(100) NOT NULL,
    note               VARCHAR(500),
    date_added         TIMESTAMP    NOT NULL,
    match_state        VARCHAR(50)  NOT NULL,
    scheduled_time_str VARCHAR(100),
    series_type        VARCHAR      NOT NULL
);

CREATE TABLE match_setup_step (
    id           SERIAL         PRIMARY KEY,
    match_id     INT            NOT NULL REFERENCES matches (id),
    step_type    VARCHAR(50)    NOT NULL,
    team_role_id BIGINT         NOT NULL,
    map          VARCHAR(100)
);

CREATE TABLE series_map (
    id                         SERIAL       PRIMARY KEY,
    match_id                   INT          NOT NULL REFERENCES matches (id),
    map                        VARCHAR(100) NOT NULL,
    picked_by_role_id          BIGINT       NOT NULL,
    start_attack_team_role_id  BIGINT,
    start_defense_team_role_id BIGINT
);

CREATE TABLE maps (
    name VARCHAR(100) NOT NULL UNIQUE
);

CREATE TABLE match_servers (
    region_label VARCHAR(100) NOT NULL UNIQUE,
    server_id    VARCHAR      NOT NULL
);

CREATE TABLE gslt_tokens (
    token   VARCHAR NOT NULL UNIQUE,
    in_use  BOOL    NOT NULL
);
