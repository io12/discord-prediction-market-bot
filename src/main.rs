mod commands;
mod money;
mod prediction_market;
mod share_quantity;

use anyhow::Error;
use poise::futures_util::lock::Mutex;
use poise::serenity_prelude as serenity;
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
    let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
    let intents = serenity::GatewayIntents::non_privileged();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: {
                use commands::*;
                vec![
                    help(),
                    balance(),
                    balances(),
                    portfolio(),
                    create_market(),
                    list_markets(),
                    show_market(),
                    resolve_market(),
                    buy(),
                    sell(),
                    tip(),
                    register(),
                    input_time(),
                ]
            },
            post_command: |ctx| Box::pin(save_state(ctx)),
            ..Default::default()
        })
        .setup(|_ctx, _ready, _framework| Box::pin(async move { Ok(Mutex::new(load_state())) }))
        .build();

    serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .unwrap()
        .start()
        .await
        .unwrap();
}
