use crate::{
    prediction_market::{Balance, MarketId, MarketInfo, ShareKind, UserShareBalance},
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
            "Creator: {}\nDescription: {}\n\nPositions:\n{}",
            Mention::User(market_info.creator),
            market_info.description,
            market_info
                .num_user_shares
                .into_iter()
                .map(|(user_id, UserShareBalance { kind, quantity })| format!(
                    "{} - {:.2} {}",
                    Mention::User(user_id),
                    quantity,
                    kind
                ))
                .collect::<Vec<String>>()
                .join("\n")
        ),
        false,
    )
}

async fn autocomplete_market(
    ctx: Context<'_>,
    _: &str,
) -> Vec<poise::AutocompleteChoice<MarketId>> {
    let economy = ctx.data().lock().await;
    economy
        .list_markets()
        .into_iter()
        .map(
            |MarketInfo {
                 market_id,
                 question,
                 ..
             }| poise::AutocompleteChoice {
                name: question,
                value: market_id,
            },
        )
        .collect()
}

/// Get help on how to use this bot
#[poise::command(slash_command, prefix_command)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Command to get help on"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> Result<()> {
    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration::default(),
    )
    .await?;
    Ok(())
}

/// Get the balance of a user
#[poise::command(slash_command, prefix_command, ephemeral)]
pub async fn balance(
    ctx: Context<'_>,
    #[description = "User to get the balance of (default is you)"] user: Option<User>,
) -> Result<()> {
    let user = user.as_ref().unwrap_or_else(|| ctx.author());
    let economy = ctx.data().lock().await;
    let response = format!("Your balance is ${:.2}", economy.balance(user.id));
    ctx.say(response).await?;
    Ok(())
}

/// Get the portfolio of a user
#[poise::command(slash_command, prefix_command, ephemeral)]
pub async fn portfolio(
    ctx: Context<'_>,
    #[description = "User to get the portfolio of (default is you)"] user: Option<User>,
) -> Result<()> {
    let user = user.as_ref().unwrap_or_else(|| ctx.author());
    let economy = ctx.data().lock().await;
    let portfolio = economy.portfolio(user.id);
    ctx.send(|f| {
        f.embed(|f| {
            f.title(format!("{}'s portfolio", user.name))
                .field("Cash", format!("${:.2}", portfolio.cash), true)
                .fields(
                    portfolio
                        .market_positions
                        .into_iter()
                        .map(|(question, position)| {
                            (
                                question,
                                format!("{:.2} {} shares", position.quantity, position.kind),
                                false,
                            )
                        }),
                )
        })
    })
    .await?;
    Ok(())
}

/// Create a market (costs $50)
#[poise::command(slash_command, prefix_command)]
pub async fn create_market(
    ctx: Context<'_>,
    #[description = "Question the market asks"] question: String,
    #[description = "Description of market, including detailed resolution criteria"]
    description: String,
) -> Result<()> {
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

/// Display a list of active markets
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

/// Resolve one of your markets
#[poise::command(slash_command, prefix_command)]
pub async fn resolve_market(
    ctx: Context<'_>,
    #[description = "ID of market to resolve"]
    #[autocomplete = "autocomplete_market"]
    market_id: MarketId,
    #[description = "Outcome to resolve to"] outcome: ShareKind,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, market_info) = economy.resolve_market(ctx.author().id, market_id, outcome)?;
    *economy = new_economy;
    ctx.send(|f| {
        f.embed(|f| {
            f.title(format!("Resolved market {outcome}:"))
                .fields(std::iter::once(market_info_to_field(market_info)))
        })
    })
    .await?;
    Ok(())
}

/// Sell your shares
#[poise::command(slash_command, prefix_command)]
pub async fn sell(
    ctx: Context<'_>,
    #[description = "ID of market to sell shares in"]
    #[autocomplete = "autocomplete_market"]
    market_id: MarketId,
    #[description = "Amount to sell (default is all of your shares)"] sell_amount: Option<Balance>,
    #[description = "Reason you are selling"] reason: Option<String>,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, num_shares_sold, sale_price) =
        economy.sell(ctx.author().id, market_id, sell_amount)?;
    *economy = new_economy;
    ctx.send(|f| {
        f.embed(|f| {
            let f = f
                .title("Sell")
                .field("Shares sold", num_shares_sold, true)
                .field("Sale price", sale_price, true)
                .field("Market", market_id, true);
            match reason {
                None => f,
                Some(reason) => f.field("Reason", reason, true),
            }
        })
    })
    .await?;
    Ok(())
}

/// Buy shares
#[poise::command(slash_command, prefix_command)]
pub async fn buy(
    ctx: Context<'_>,
    #[description = "ID of market to buy shares in"]
    #[autocomplete = "autocomplete_market"]
    market_id: MarketId,
    #[description = "Amount of money to use for buying shares"]
    #[min = 0]
    purchase_price: Balance,
    #[description = "Type of share you want to buy"] share_kind: ShareKind,
    #[description = "Reason you are buying"] reason: Option<String>,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, shares_received) =
        economy.buy(ctx.author().id, market_id, purchase_price, share_kind)?;
    *economy = new_economy;
    ctx.send(|f| {
        f.embed(|f| {
            let f = f
                .title("Buy")
                .field("Shares bought", shares_received, true)
                .field("Buy price", purchase_price, true)
                .field("Market", market_id, true);
            match reason {
                None => f,
                Some(reason) => f.field("Reason", reason, true),
            }
        })
    })
    .await?;
    Ok(())
}

/// Send a tip to another user
#[poise::command(slash_command, prefix_command)]
pub async fn tip(
    ctx: Context<'_>,
    #[description = "User to send your tip to"] user_to_tip: User,
    #[description = "Amount of money to send"] amount: Balance,
    #[description = "Reason for tip"] reason: Option<String>,
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
