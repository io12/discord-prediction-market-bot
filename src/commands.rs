use crate::{
    prediction_market::{Balance, MarketId, MarketInfo, ShareKind},
    Context,
};
use anyhow::Result;
use poise::serenity_prelude::{Mention, User, UserId};

fn market_info_to_field(market_info: MarketInfo<UserId>) -> (String, String, bool) {
    (
        format!(
            "__{}__   {}   **{}**_%_",
            market_info.market_id, market_info.question, market_info.probability
        ),
        format!(
            "Creator: {}\nDescription: {}",
            Mention::User(market_info.creator),
            market_info.description,
        ),
        false,
    )
}

#[poise::command(slash_command, prefix_command, ephemeral)]
pub async fn balance(ctx: Context<'_>, user: Option<User>) -> Result<()> {
    let user = user.as_ref().unwrap_or_else(|| ctx.author());
    let economy = ctx.data().lock().await;
    let response = format!("Your balance is ${:.2}", economy.balance(user.id));
    ctx.say(response).await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn create_market(ctx: Context<'_>, question: String, description: String) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, market_info) =
        economy.create_market(ctx.author().id, question, description)?;
    *economy = new_economy;
    ctx.send(|f| {
        f.embed(|f| {
            f.title("Created market:")
                .fields(std::iter::once(market_info_to_field(market_info)))
        })
    })
    .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn list_markets(ctx: Context<'_>) -> Result<()> {
    let economy = ctx.data().lock().await;
    ctx.send(|f| {
        f.embed(|f| {
            f.title("Markets")
                .fields(economy.list_markets().map(market_info_to_field))
        })
    })
    .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn resolve_market(
    ctx: Context<'_>,
    market_id: MarketId,
    outcome: ShareKind,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let new_economy = economy.resolve_market(ctx.author().id, market_id, outcome)?;
    *economy = new_economy;
    ctx.say("resolved market successfully (TODO: show stats)")
        .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn sell(
    ctx: Context<'_>,
    market_id: MarketId,
    sell_amount: Option<Balance>,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, num_shares_sold, sale_price) =
        economy.sell(ctx.author().id, market_id, sell_amount)?;
    *economy = new_economy;
    ctx.say(format!(
        "Sold {num_shares_sold:.2} shares for ${sale_price:.2}"
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn buy(
    ctx: Context<'_>,
    market_id: MarketId,
    #[min = 0] purchase_price: Balance,
    share_kind: ShareKind,
    reason: Option<String>,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, shares_received) =
        economy.buy(ctx.author().id, market_id, purchase_price, share_kind)?;
    *economy = new_economy;
    ctx.say(format!(
        "Bought {shares_received:.2} {share_kind} shares for ${purchase_price:.2}{}",
        match reason {
            None => String::new(),
            Some(reason) => format!(" because \"{reason}\""),
        }
    ))
    .await?;
    Ok(())
}

#[poise::command(slash_command, prefix_command)]
pub async fn tip(
    ctx: Context<'_>,
    user_to_tip: User,
    amount: Balance,
    reason: Option<String>,
) -> Result<()> {
    let user_to_tip = user_to_tip.id;
    let mut economy = ctx.data().lock().await;
    let new_economy = economy.tip(ctx.author().id, user_to_tip, amount)?;
    *economy = new_economy;
    ctx.say(format!(
        "Tipped ${amount:.2} to {}{}",
        Mention::User(user_to_tip),
        match reason {
            None => String::new(),
            Some(reason) => format!(" because \"{reason}\""),
        }
    ))
    .await?;
    Ok(())
}
