mod commands;
mod prediction_market;

use anyhow::Error;
use poise::futures_util::lock::Mutex;
use poise::serenity_prelude::{self as serenity, GuildId};
use std::fs::File;

type Context<'a> = poise::Context<'a, Mutex<Economy>, Error>;
type Economy = crate::prediction_market::Economy<serenity::UserId>;

fn load_state() -> Economy {
    match File::open("state.json") {
        Ok(file) => serde_json::from_reader(file).unwrap(),
        Err(_) => Economy::new(),
    }
}

async fn save_state(ctx: Context<'_>) {
    let economy = ctx.data().lock().await;
    let file = File::create("state.json").expect("failed creating state.json");
    serde_json::to_writer(file, &*economy).expect("failed writing economy to state.json");
}

#[tokio::main]
async fn main() {
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: {
                use commands::*;
                vec![
                    help(),
                    balance(),
                    portfolio(),
                    create_market(),
                    list_markets(),
                    resolve_market(),
                    buy(),
                    sell(),
                    tip(),
                ]
            },
            post_command: |ctx| Box::pin(save_state(ctx)),
            ..Default::default()
        })
        .token(std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN"))
        .intents(serenity::GatewayIntents::non_privileged())
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_in_guild(
                    ctx,
                    &framework.options().commands,
                    GuildId(848698959282569257),
                )
                .await?;
                Ok(Mutex::new(load_state()))
            })
        });

    framework.run().await.unwrap();
}
