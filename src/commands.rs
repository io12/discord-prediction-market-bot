use crate::{
    money::Money,
    prediction_market::{Market, MarketId, ShareKind, UserShareBalance},
    share_quantity::ShareQuantity,
    Context, Economy,
};
use anyhow::{Context as AnyhowContext, Result};
use poise::serenity_prelude::{Color, Mention, Mentionable, User, UserId};

impl ShareKind {
    fn color(&self) -> Color {
        match self {
            Self::Yes => Color::DARK_GREEN,
            Self::No => Color::RED,
        }
    }
}

fn market_to_field(market: &Market<UserId>) -> (String, String, bool) {
    (
        format!(
            "__{}__   {}   **{}**_%_",
            market.id,
            market.question,
            market.probability()
        ),
        format!(
            "{}Creator: {}\nDescription: {}\n\nPositions:\n{}",
            match market.close_timestamp {
                None => String::new(),
                Some(close_timestamp) => format!(
                    "{}: <t:{close_timestamp}:R>, <t:{close_timestamp}:F>\n",
                    if market.is_open() { "Closes" } else { "Closed" }
                ),
            },
            Mention::User(market.creator),
            market.description,
            market
                .num_user_shares
                .iter()
                .map(|(user_id, UserShareBalance { kind, quantity })| format!(
                    "{} - {} {}",
                    Mention::User(*user_id),
                    quantity,
                    kind
                ))
                .collect::<Vec<String>>()
                .join("\n")
        ),
        false,
    )
}

fn make_matcher() -> impl fuzzy_matcher::FuzzyMatcher {
    fuzzy_matcher::skim::SkimMatcherV2::default().ignore_case()
}

async fn autocomplete_market(
    ctx: Context<'_>,
    prefix: &str,
) -> Vec<poise::AutocompleteChoice<MarketId>> {
    use fuzzy_matcher::FuzzyMatcher;
    let matcher = make_matcher();
    let economy = ctx.data().lock().await;
    economy
        .list_markets()
        .into_iter()
        .filter_map(|Market { id, question, .. }| {
            matcher
                .fuzzy_match(question, prefix)
                .map(|_| poise::AutocompleteChoice {
                    name: question.clone(),
                    value: *id,
                })
        })
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

/// Get the balances of all users
#[poise::command(slash_command, prefix_command)]
pub async fn balances(ctx: Context<'_>) -> Result<()> {
    let economy = ctx.data().lock().await;
    ctx.send(|f| {
        f.embed(|f| {
            f.color(Color::DARK_GOLD).title("User balances").fields(
                economy
                    .balances()
                    .into_iter()
                    .enumerate()
                    .map(|(i, (user_id, balance))| {
                        let mention = Mention::User(user_id);
                        (i + 1, format!("{mention} {balance}"), true)
                    }),
            )
        })
    })
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
        "{}'s balance is {}",
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
            f.color(Color::TEAL)
                .title(format!("{}'s portfolio", user.name))
                .field("Cash", portfolio.cash, true)
                .fields(
                    portfolio
                        .market_positions
                        .into_iter()
                        .map(|(question, position)| {
                            (
                                question,
                                format!("{} {} shares", position.quantity, position.kind),
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
    #[description = "Date and/or time the market closes (default is none)"]
    close_date_and_time: Option<String>,
    #[description = "Time zone to use for market close time (default is US/Eastern)"]
    time_zone: Option<String>,
) -> Result<()> {
    let time_zone = match time_zone {
        Some(time_zone) => time_zone
            .parse::<chrono_tz::Tz>()
            .ok()
            .context("invalid time zone")?,
        None => chrono_tz::US::Eastern,
    };
    let close_date_and_time = close_date_and_time
        .map(|s| {
            chrono_english::parse_date_string(
                &s,
                chrono::Local::now().with_timezone(&time_zone),
                chrono_english::Dialect::Us,
            )
        })
        .transpose()
        .context("failed parsing close date and time")?;
    let close_timestamp = close_date_and_time.map(|date_time| date_time.timestamp());
    let mut economy = ctx.data().lock().await;
    let (new_economy, market_id) =
        economy.create_market(ctx.author().id, question, description, close_timestamp)?;
    *economy = new_economy;
    let market = economy.market(market_id)?;
    ctx.send(|f| {
        f.embed(|f| {
            f.color(Color::GOLD)
                .title("Created market:")
                .fields(std::iter::once(market_to_field(market)))
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
            f.color(Color::DARK_BLUE)
                .title("Markets")
                .fields(economy.list_markets().map(market_to_field))
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
    let (new_economy, market) = economy.resolve_market(ctx.author().id, market, outcome)?;
    *economy = new_economy;
    ctx.send(|f| {
        f.embed(|f| {
            f.color(outcome.color())
                .title(format!("Resolved market {outcome}:"))
                .fields(std::iter::once(market_to_field(&market)))
        })
    })
    .await?;
    Ok(())
}

fn probability_change_string(
    old_economy: &Economy,
    new_economy: &Economy,
    market_id: MarketId,
) -> Result<String> {
    let old_prob = old_economy.market(market_id)?.probability();
    let new_prob = new_economy.market(market_id)?.probability();
    Ok(format!("{old_prob}% → {new_prob}%"))
}

/// Sell your shares
#[poise::command(slash_command, prefix_command)]
pub async fn sell(
    ctx: Context<'_>,
    #[description = "ID of market to sell shares in"]
    #[autocomplete = "autocomplete_market"]
    market: MarketId,
    #[description = "Amount to sell (default is all of your shares)"] sell_amount: Option<f64>,
    #[description = "Reason you are selling"] reason: Option<String>,
) -> Result<()> {
    let sell_amount = sell_amount.map(ShareQuantity);
    let mut economy = ctx.data().lock().await;
    let (new_economy, num_shares_sold, sale_price) =
        economy.sell(ctx.author().id, market, sell_amount)?;
    let prob_change = probability_change_string(&economy, &new_economy, market)?;
    *economy = new_economy;
    let market_name = &economy.market(market)?.question;
    ctx.send(|f| {
        f.embed(|f| {
            let f = f
                .color(Color::BLITZ_BLUE)
                .title("Sell")
                .field("Shares sold", num_shares_sold, true)
                .field("Sale price", sale_price, true)
                .field("Probability change", prob_change, true)
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
    purchase_price: f64,
    #[description = "Type of share you want to buy"] share_kind: ShareKind,
    #[description = "Reason you are buying"] reason: Option<String>,
) -> Result<()> {
    let purchase_price = Money(purchase_price);
    let mut economy = ctx.data().lock().await;
    let (new_economy, shares_received) =
        economy.buy(ctx.author().id, market, purchase_price, share_kind)?;
    let prob_change = probability_change_string(&economy, &new_economy, market)?;
    *economy = new_economy;
    let market_name = &economy.market(market)?.question;
    ctx.send(|f| {
        f.embed(|f| {
            let f = f
                .color(share_kind.color())
                .title(format!("Buy {share_kind}"))
                .field("Shares bought", shares_received, true)
                .field("Buy price", purchase_price, true)
                .field("Probability change", prob_change, true)
                .field(
                    format!("Profit if {share_kind}"),
                    format!(
                        "+{:.0}%",
                        (shares_received.0 / purchase_price.0 - 1.0) * 100.0
                    ),
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
    #[description = "Amount of money to send"] amount: f64,
    #[description = "Reason for tip"] reason: Option<String>,
) -> Result<()> {
    let amount = Money(amount);
    let mut economy = ctx.data().lock().await;
    let new_economy = economy.tip(ctx.author().id, user_to_tip.id, amount)?;
    *economy = new_economy;
    ctx.say(format!(
        "Tipped {amount} to {}{}",
        user_to_tip.mention(),
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
    let matcher = make_matcher();
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

/// Register slash commands
#[poise::command(slash_command, prefix_command, owners_only)]
pub async fn register(ctx: Context<'_>) -> Result<()> {
    poise::builtins::register_application_commands_buttons(ctx).await?;
    Ok(())
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
            f.color(Color::BLURPLE)
                .title("Time input test")
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
