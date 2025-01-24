use crate::{
    money::Money,
    prediction_market::{Market, MarketId, ResolveOutcome, ShareKind, TransactionInfo},
    share_quantity::ShareQuantity,
    Context, Economy,
};
use anyhow::{Context as AnyhowContext, Result};
use poise::serenity_prelude::{
    AutocompleteChoice, ButtonStyle, Color, ComponentInteractionCollector, CreateActionRow,
    CreateButton, CreateEmbed, EditInteractionResponse, Mention, Mentionable, User, UserId,
};

impl ShareKind {
    fn color(&self) -> Color {
        match self {
            Self::Yes => Color::DARK_GREEN,
            Self::No => Color::RED,
        }
    }
}

impl ResolveOutcome {
    fn color(&self) -> Color {
        match self {
            ResolveOutcome::Yes => ShareKind::Yes.color(),
            ResolveOutcome::No => ShareKind::No.color(),
            ResolveOutcome::Undo => Color::LIGHTER_GREY,
        }
    }
}

fn market_to_brief_field(market: &Market<UserId>) -> (String, String, bool) {
    let creator = Mention::User(market.creator);
    let close_text = match market.close_timestamp {
        None => String::new(),
        Some(close_timestamp) => format!(
            "\n{} <t:{close_timestamp}:R>, <t:{close_timestamp}:F>",
            if market.is_open() { "Closes" } else { "Closed" }
        ),
    };
    (
        format!(
            "__{}__   {}   **{}**_%_",
            market.id,
            market.question,
            market.probability()
        ),
        format!("{creator}{close_text}"),
        false,
    )
}

fn market_positions_string(market: &Market<UserId>) -> String {
    market
        .num_user_shares
        .iter()
        .map(|(user_id, kind_quantity)| format!("{} - {kind_quantity}", Mention::User(*user_id)))
        .collect::<Vec<String>>()
        .join("\n")
}

fn market_transactions_string(market: &Market<UserId>) -> String {
    match &market.transaction_history {
        Some(hist) => hist
            .iter()
            .map(
                |TransactionInfo {
                     user,
                     kind,
                     shares,
                     money,
                     new_probability,
                 }| {
                    let user = Mention::User(*user);
                    format!("{user} {kind} {shares} for {money} | {new_probability}%")
                },
            )
            .collect::<Vec<String>>()
            .join("\n"),
        None => "_Market was created before transaction history was implemented_".to_string(),
    }
}

fn market_to_descriptive_fields(market: &Market<UserId>) -> [(String, String, bool); 4] {
    [
        market_to_brief_field(market),
        ("Description".into(), market.description.clone(), false),
        ("Positions".into(), market_positions_string(market), false),
        (
            "Transactions".into(),
            market_transactions_string(market),
            false,
        ),
    ]
}

fn make_matcher() -> impl fuzzy_matcher::FuzzyMatcher {
    fuzzy_matcher::skim::SkimMatcherV2::default().ignore_case()
}

async fn autocomplete_market(ctx: Context<'_>, prefix: &str) -> Vec<AutocompleteChoice> {
    use fuzzy_matcher::FuzzyMatcher;
    let matcher = make_matcher();
    let economy = ctx.data().lock().await;
    economy
        .list_markets()
        .filter_map(|Market { id, question, .. }| {
            matcher
                .fuzzy_match(question, prefix)
                .map(|_| AutocompleteChoice::new(question, *id))
        })
        .collect()
}

async fn autocomplete_users_markets(ctx: Context<'_>, prefix: &str) -> Vec<AutocompleteChoice> {
    use fuzzy_matcher::FuzzyMatcher;
    let matcher = make_matcher();
    let economy = ctx.data().lock().await;
    economy
        .list_markets()
        .filter_map(
            |Market {
                 id,
                 creator,
                 question,
                 ..
             }| {
                if *creator == ctx.author().id {
                    matcher
                        .fuzzy_match(question, prefix)
                        .map(|_| AutocompleteChoice::new(question, *id))
                } else {
                    None
                }
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

/// Get the balances of all users
#[poise::command(slash_command, prefix_command)]
pub async fn balances(ctx: Context<'_>) -> Result<()> {
    let economy = ctx.data().lock().await;
    ctx.send(
        poise::CreateReply::default().embed(
            CreateEmbed::new()
                .color(Color::DARK_GOLD)
                .title("User balances")
                .fields(economy.balances().into_iter().enumerate().map(
                    |(i, (user_id, balance))| {
                        let num = i + 1;
                        let mention = Mention::User(user_id);
                        (format!("{num}"), format!("{mention} {balance}"), true)
                    },
                )),
        ),
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
    ctx.send(
        poise::CreateReply::default().embed(
            CreateEmbed::new()
                .color(Color::TEAL)
                .title(format!("{}'s portfolio", user.name))
                .field("Cash", format!("{}", portfolio.cash), true)
                .fields(
                    portfolio
                        .market_positions
                        .into_iter()
                        .map(|(question, kind_quantity)| {
                            (question, format!("{kind_quantity} shares"), false)
                        }),
                ),
        ),
    )
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
    let market = new_economy.market(market_id)?;
    ctx.send(
        poise::CreateReply::default().embed(
            CreateEmbed::new()
                .color(Color::GOLD)
                .title("Created market:")
                .fields(market_to_descriptive_fields(market)),
        ),
    )
    .await?;
    *economy = new_economy;
    Ok(())
}

/// Display a list of active markets
#[poise::command(slash_command, prefix_command)]
pub async fn list_markets(ctx: Context<'_>) -> Result<()> {
    let economy = ctx.data().lock().await;
    ctx.send(
        poise::CreateReply::default().embed(
            CreateEmbed::new()
                .color(Color::DARK_BLUE)
                .title("Markets")
                .fields(economy.list_markets().map(market_to_brief_field)),
        ),
    )
    .await?;
    Ok(())
}

/// Show a market
#[poise::command(slash_command, prefix_command)]
pub async fn show_market(
    ctx: Context<'_>,
    #[description = "Market to show"]
    #[autocomplete = "autocomplete_market"]
    market: MarketId,
) -> Result<()> {
    let economy = ctx.data().lock().await;
    let market = economy.market(market)?;
    ctx.send(
        poise::CreateReply::default().embed(
            CreateEmbed::new()
                .color(Color::DARK_BLUE)
                .title("Market")
                .fields(market_to_descriptive_fields(market)),
        ),
    )
    .await?;
    Ok(())
}

/// Resolve one of your markets
#[poise::command(slash_command, prefix_command)]
pub async fn resolve_market(
    ctx: Context<'_>,
    #[description = "Market to resolve"]
    #[autocomplete = "autocomplete_users_markets"]
    market: MarketId,
    #[description = "Outcome to resolve to"] outcome: ResolveOutcome,
) -> Result<()> {
    let mut economy = ctx.data().lock().await;
    let (new_economy, market) = economy.resolve_market(ctx.author().id, market, outcome)?;
    ctx.send(
        poise::CreateReply::default().embed(
            CreateEmbed::new()
                .color(outcome.color())
                .title(format!("Resolved market {outcome}:"))
                .fields(market_to_descriptive_fields(&market)),
        ),
    )
    .await?;
    *economy = new_economy;
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
    #[description = "Market to sell shares in"]
    #[autocomplete = "autocomplete_market"]
    market: MarketId,
    #[description = "Amount to sell (default is all of your shares)"] sell_amount: Option<f64>,
    #[description = "Reason you are selling"] reason: Option<String>,
) -> Result<()> {
    let sell_amount = sell_amount.map(ShareQuantity);
    let mut economy = ctx.data().lock().await;
    let (new_economy, shares_sold, sale_price) =
        economy.sell(ctx.author().id, market, sell_amount)?;
    let prob_change = probability_change_string(&economy, &new_economy, market)?;
    let market_name = &economy.market(market)?.question;
    let embed = CreateEmbed::new()
        .color(Color::BLITZ_BLUE)
        .title(format!("Sell {}", shares_sold.kind))
        .field("Shares sold", shares_sold.to_string(), true)
        .field("Sale price", sale_price.to_string(), true)
        .field("Probability change", prob_change, true)
        .field("Market", market_name, true);
    let embed = match reason {
        None => embed,
        Some(reason) => embed.field("Reason", reason, true),
    };
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    *economy = new_economy;
    Ok(())
}

/// Buy shares
#[poise::command(slash_command, prefix_command)]
pub async fn buy(
    ctx: Context<'_>,
    #[description = "Market to buy shares in"]
    #[autocomplete = "autocomplete_market"]
    market: MarketId,
    #[description = "Amount of money to use for buying shares"]
    #[min = 0]
    purchase_price: f64,
    #[description = "Type of share you want to buy"] share_kind: ShareKind,
    #[description = "Reason you are buying"] reason: Option<String>,
) -> Result<()> {
    let purchase_price = Money(purchase_price);
    let id = ctx.author().id;
    let (old_economy, (new_economy, shares_received)) = {
        let economy = ctx.data().lock().await;
        (
            economy.clone(),
            economy.buy(id, market, purchase_price, share_kind)?,
        )
    };
    let old_market = old_economy.market(market)?;
    let prob_change = probability_change_string(&old_economy, &new_economy, market)?;
    let market_name = &old_market.question;

    let embed = CreateEmbed::new()
        .color(share_kind.color())
        .title(format!("Buy {share_kind}"))
        .field("Shares bought", shares_received.to_string(), true)
        .field("Buy price", purchase_price.to_string(), true)
        .field("Probability change", prob_change, true)
        .field(
            format!("Profit if {share_kind}"),
            format!(
                "+{} (+{:.0}%)",
                Money(shares_received.0 - purchase_price.0),
                (shares_received.0 / purchase_price.0 - 1.0) * 100.0,
            ),
            true,
        )
        .field("Market", market_name, true);

    let embed = match reason {
        None => embed,
        Some(reason) => embed.field("Reason", reason, true),
    };

    let buttons = vec![CreateActionRow::Buttons(vec![
        CreateButton::new("confirm")
            .label("Confirm")
            .style(ButtonStyle::Success),
        CreateButton::new("deny")
            .label("Deny")
            .style(ButtonStyle::Danger),
    ])];

    ctx.send(
        poise::CreateReply::default()
            .embed(embed)
            .components(buttons)
            .ephemeral(true),
    )
    .await?;

    while let Some(mci) = ComponentInteractionCollector::new(ctx.serenity_context()).await {
        match mci.data.custom_id.as_str() {
            "confirm" => {
                let mut economy = ctx.data().lock().await;
                if economy.market(market)? == old_market {
                    mci.edit_response(
                        ctx.http(),
                        EditInteractionResponse::new().content("Confirmed."),
                    )
                    .await?;
                    let (new_economy, _) = economy.buy(id, market, purchase_price, share_kind)?;
                    *economy = new_economy;
                } else {
                    mci.edit_response(
                        ctx.http(),
                        EditInteractionResponse::new().content("Market changed. Try again."),
                    )
                    .await?;
                }
            }
            "deny" => {
                mci.edit_response(
                    ctx.http(),
                    EditInteractionResponse::new().content("Denied."),
                )
                .await?;
            }
            _ => {}
        }
    }

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
    ctx.say(format!(
        "Tipped {amount} to {}{}",
        user_to_tip.mention(),
        match reason {
            None => String::new(),
            Some(reason) => format!(" because \"{reason}\""),
        }
    ))
    .await?;
    *economy = new_economy;
    Ok(())
}

async fn autocomplete_tz(_: Context<'_>, prefix: &str) -> Vec<AutocompleteChoice> {
    use fuzzy_matcher::FuzzyMatcher;
    let matcher = make_matcher();
    chrono_tz::TZ_VARIANTS
        .into_iter()
        .filter_map(|tz| {
            matcher
                .fuzzy_match(tz.name(), prefix)
                .map(|_| AutocompleteChoice::new(tz.to_string(), tz.name()))
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
    ctx.send(
        poise::CreateReply::default().embed(
            CreateEmbed::new()
                .color(Color::BLURPLE)
                .title("Time input test")
                .field("Date/time input string", date_time, true)
                .field("Timezone", timezone.name(), true)
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
                ),
        ),
    )
    .await?;
    Ok(())
}
