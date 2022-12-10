use crate::{
    prediction_market::{Balance, MarketId, MarketInfo, ShareKind, UserShareBalance},
    Context,
};
use anyhow::Result;
use poise::serenity_prelude::{Mention, Mentionable, User, UserId};

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
    prefix: &str,
) -> Vec<poise::AutocompleteChoice<MarketId>> {
    use fuzzy_matcher::FuzzyMatcher;
    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
    let economy = ctx.data().lock().await;
    economy
        .list_markets()
        .into_iter()
        .filter_map(
            |MarketInfo {
                 market_id,
                 question,
                 ..
             }| {
                matcher
                    .fuzzy_match(&question, prefix)
                    .map(|_| poise::AutocompleteChoice {
                        name: question,
                        value: market_id,
                    })
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
    let response = format!(
        "{}'s balance is ${:.2}",
        user.mention(),
        economy.balance(user.id)
    );
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
    market: MarketId,
    #[description = "Outcome to resolve to"] outcome: ShareKind,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, market_info) = economy.resolve_market(ctx.author().id, market, outcome)?;
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
    market: MarketId,
    #[description = "Amount to sell (default is all of your shares)"] sell_amount: Option<Balance>,
    #[description = "Reason you are selling"] reason: Option<String>,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, num_shares_sold, sale_price) =
        economy.sell(ctx.author().id, market, sell_amount)?;
    let old_prob = economy.market_probability(market)?;
    *economy = new_economy;
    let new_prob = economy.market_probability(market)?;
    let market_name = economy.market_name(market)?;
    ctx.send(|f| {
        f.embed(|f| {
            let f = f
                .title("Sell")
                .field("Shares sold", format!("{num_shares_sold:.2}"), true)
                .field("Sale price", format!("${sale_price:.2}"), true)
                .field(
                    "Probability change",
                    format!("{old_prob}% → {new_prob}%"),
                    true,
                )
                .field("Market", market_name, true);
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
    market: MarketId,
    #[description = "Amount of money to use for buying shares"]
    #[min = 0]
    purchase_price: Balance,
    #[description = "Type of share you want to buy"] share_kind: ShareKind,
    #[description = "Reason you are buying"] reason: Option<String>,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, shares_received) =
        economy.buy(ctx.author().id, market, purchase_price, share_kind)?;
    let old_prob = economy.market_probability(market)?;
    *economy = new_economy;
    let new_prob = economy.market_probability(market)?;
    let market_name = economy.market_name(market)?;
    ctx.send(|f| {
        f.embed(|f| {
            let f = f
                .title(format!("Buy {share_kind}"))
                .field("Shares bought", format!("{shares_received:.2}"), true)
                .field("Buy price", format!("${purchase_price:.2}"), true)
                .field(
                    "Probability change",
                    format!("{old_prob}% → {new_prob}%"),
                    true,
                )
                .field(
                    format!("Profit if {share_kind}"),
                    format!("+{:.0}%", (shares_received / purchase_price - 1.0) * 100.0),
                    true,
                )
                .field("Market", market_name, true);
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

async fn autocomplete_tz(
    _: Context<'_>,
    prefix: &str,
) -> Vec<poise::AutocompleteChoice<&'static str>> {
    use fuzzy_matcher::FuzzyMatcher;
    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
    chrono_tz::TZ_VARIANTS
        .into_iter()
        .filter_map(|tz| {
            matcher
                .fuzzy_match(tz.name(), prefix)
                .map(|_| poise::AutocompleteChoice {
                    name: tz.to_string(),
                    value: tz.name(),
                })
        })
        .collect()
}

/// Test time input
#[poise::command(slash_command, prefix_command)]
pub async fn input_time(
    ctx: Context<'_>,
    date_time: String,
    #[autocomplete = "autocomplete_tz"] timezone: String,
) -> Result<()> {
    let timezone = timezone.parse::<chrono_tz::Tz>().unwrap();
    let date_time_parsed = chrono_english::parse_date_string(
        &date_time,
        chrono::Local::now().with_timezone(&timezone),
        chrono_english::Dialect::Us,
    )?;
    let timestamp = date_time_parsed.timestamp();
    ctx.send(|f| {
        f.embed(|f| {
            f.title("Time input test")
                .field("Date/time input string", date_time, true)
                .field("Timezone", timezone, true)
                .field(
                    "Discord timestamp (short time)",
                    format!("<t:{timestamp}:t>"),
                    true,
                )
                .field(
                    "Discord timestamp (long time)",
                    format!("<t:{timestamp}:T>"),
                    true,
                )
                .field(
                    "Discord timestamp (short date)",
                    format!("<t:{timestamp}:d>"),
                    true,
                )
                .field(
                    "Discord timestamp (long date)",
                    format!("<t:{timestamp}:D>"),
                    true,
                )
                .field(
                    "Discord timestamp (short date/time)",
                    format!("<t:{timestamp}:f>"),
                    true,
                )
                .field(
                    "Discord timestamp (long date/time)",
                    format!("<t:{timestamp}:F>"),
                    true,
                )
                .field(
                    "Discord timestamp (relative time)",
                    format!("<t:{timestamp}:R>"),
                    true,
                )
        })
    })
    .await?;
    Ok(())
}
