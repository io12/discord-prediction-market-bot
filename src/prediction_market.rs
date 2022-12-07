use anyhow::{bail, ensure, Context, Result};
use im::ordmap::OrdMap;
use poise::ChoiceParameter;
use serde::{Deserialize, Serialize};

pub type Balance = f64;
pub type MarketId = u64;

const USER_START_BALANCE: Balance = 1000.0;
const MARKET_CREATION_COST: Balance = 50.0;

#[derive(Clone, Serialize, Deserialize)]
pub struct Economy<UserId: Ord + Clone> {
    next_market_id: MarketId,
    user_money: OrdMap<UserId, Balance>,
    markets: OrdMap<MarketId, Market<UserId>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Market<UserId: Ord + Clone> {
    creator: UserId,
    question: String,
    description: String,
    y: Balance,
    n: Balance,
    num_user_shares: OrdMap<UserId, UserShareBalance>,
}

pub struct MarketInfo<UserId> {
    pub market_id: MarketId,
    pub question: String,
    pub probability: u8,
    pub creator: UserId,
    pub description: String,
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, ChoiceParameter)]
pub enum ShareKind {
    #[name = "YES"]
    Yes,
    #[name = "NO"]
    No,
}

#[derive(Clone, Serialize, Deserialize)]
struct UserShareBalance {
    kind: ShareKind,
    quantity: Balance,
}

impl<UserId: Ord + Clone> Market<UserId> {
    fn new(creator: UserId, question: String, description: String) -> Self {
        Market {
            creator,
            question,
            description,
            y: MARKET_CREATION_COST,
            n: MARKET_CREATION_COST,
            num_user_shares: OrdMap::new(),
        }
    }

    fn probability(&self) -> u8 {
        let p = self.n / (self.y + self.n);
        (p * 100.0) as u8
    }

    fn info(&self, market_id: MarketId) -> MarketInfo<UserId> {
        MarketInfo {
            market_id,
            question: self.question.clone(),
            probability: self.probability(),
            creator: self.creator.clone(),
            description: self.description.clone(),
        }
    }
}

impl<UserId: Ord + Clone> Economy<UserId> {
    pub fn new() -> Self {
        Self {
            next_market_id: 0,
            user_money: OrdMap::new(),
            markets: OrdMap::new(),
        }
    }

    pub fn balance(&self, user: UserId) -> Balance {
        *self.user_money.get(&user).unwrap_or(&USER_START_BALANCE)
    }

    fn balance_mut(&mut self, user: UserId) -> &mut Balance {
        self.user_money.entry(user).or_insert(USER_START_BALANCE)
    }

    pub fn create_market(
        &self,
        calling_user: UserId,
        question: String,
        description: String,
    ) -> Result<(Economy<UserId>, MarketInfo<UserId>)> {
        let mut new_economy = self.clone();

        // Create new market ID
        let market_id = new_economy.next_market_id;
        new_economy.next_market_id = market_id
            .checked_add(1)
            .context("overflow getting next market id")?;

        // Deduct market creation cost
        let user_money = new_economy.balance_mut(calling_user.clone());
        *user_money -= MARKET_CREATION_COST;
        ensure!(
            !user_money.is_sign_negative(),
            "can't afford market creation cost"
        );

        // Create market
        let market = Market::new(calling_user, question, description);
        let market_info = market.info(market_id);
        let _ = new_economy.markets.insert(market_id, market);

        Ok((new_economy, market_info))
    }

    pub fn resolve_market(
        &self,
        calling_user: UserId,
        market_id: MarketId,
        outcome: ShareKind,
    ) -> Result<Economy<UserId>> {
        let market = self
            .markets
            .get(&market_id)
            .context("market does not exist")?;
        ensure!(
            calling_user == market.creator,
            "this is someone else's market"
        );

        let mut new_economy = self.clone();

        for (user, share_balance) in market.num_user_shares.iter() {
            if share_balance.kind == outcome {
                let user_money = new_economy.balance_mut(user.clone());
                *user_money += share_balance.quantity
            }
        }

        let caller_money = new_economy.balance_mut(calling_user);
        match outcome {
            ShareKind::No => *caller_money += market.n,
            ShareKind::Yes => *caller_money += market.y,
        }

        new_economy.markets.remove(&market_id).context("market does not exist, after we already accessed it?? this definitely shouldn't happen")?;

        Ok(new_economy)
    }

    pub fn sell(
        &self,
        calling_user: UserId,
        market_id: MarketId,
        sell_amount: Option<Balance>,
    ) -> Result<(Economy<UserId>, Balance, Balance)> {
        let mut new_economy = self.clone();
        let market = new_economy
            .markets
            .get_mut(&market_id)
            .context("market does not exist")?;
        let product = market.y * market.n;
        let user_shares = market
            .num_user_shares
            .get_mut(&calling_user)
            .context("you have no shares to sell")?;
        let num_shares = &mut user_shares.quantity;
        let num_shares_to_sell = sell_amount.unwrap_or(*num_shares);
        ensure!(
            num_shares_to_sell.is_sign_positive(),
            "must sell a positive number of shares"
        );
        *num_shares -= num_shares_to_sell;
        ensure!(
            !num_shares.is_sign_negative(),
            "you are trying to sell more shares than you have"
        );
        let num_market_shares = match user_shares.kind {
            ShareKind::No => &mut market.n,
            ShareKind::Yes => &mut market.y,
        };
        *num_market_shares += num_shares_to_sell;
        let y = market.y;
        let n = market.n;
        let k = product;
        let sale_price = (y + n - ((y + n).powf(2.0) + 4.0 * (k - n * y)).sqrt()) / 2.0;
        market.n -= sale_price;
        ensure!(
            !market.n.is_sign_negative(),
            "underflow balancing market NO shares"
        );
        market.y -= sale_price;
        ensure!(
            !market.y.is_sign_negative(),
            "underflow balancing market YES shares"
        );
        let user_money = new_economy.balance_mut(calling_user);
        *user_money += sale_price;
        Ok((new_economy, num_shares_to_sell, sale_price))
    }

    pub fn buy(
        &self,
        calling_user: UserId,
        market_id: MarketId,
        purchase_price: Balance,
        share_kind: ShareKind,
    ) -> Result<(Economy<UserId>, Balance)> {
        ensure!(
            purchase_price.is_sign_positive(),
            "must buy with a positive amount of money"
        );
        let mut new_economy = self.clone();
        let user_money = new_economy.balance_mut(calling_user.clone());
        *user_money -= purchase_price;
        ensure!(
            !user_money.is_sign_negative(),
            "you can't afford that in this economy"
        );
        let market = new_economy
            .markets
            .get_mut(&market_id)
            .context("market does not exist")?;
        let product = market.y * market.n;
        market.n += purchase_price;
        market.y += purchase_price;
        let n = market.n;
        let y = market.y;
        let k = product;
        let bought_shares = match share_kind {
            ShareKind::No => {
                let bought_shares = (n * y - k) / y;
                market.n -= bought_shares;
                ensure!(
                    !market.n.is_sign_negative(),
                    "underflow subtracting NO shares for user"
                );
                bought_shares
            }
            ShareKind::Yes => {
                let bought_shares = (n * y - k) / n;
                market.y -= bought_shares;
                ensure!(
                    !market.y.is_sign_negative(),
                    "underflow subtracting YES shares for user"
                );
                bought_shares
            }
        };
        let new_user_shares = UserShareBalance {
            kind: share_kind,
            quantity: bought_shares,
        };
        match market.num_user_shares.entry(calling_user) {
            im::ordmap::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(new_user_shares);
            }
            im::ordmap::Entry::Occupied(mut occupied_entry) => {
                let user_shares = occupied_entry.get_mut();
                if user_shares.kind == new_user_shares.kind {
                    user_shares.quantity += new_user_shares.quantity;
                } else {
                    bail!("You already have shares of the other type. You should sell those first. TODO: automatically do this")
                }
            }
        }
        Ok((new_economy, bought_shares))
    }

    pub fn list_markets(&self) -> impl Iterator<Item = MarketInfo<UserId>> + '_ {
        self.markets
            .iter()
            .map(|(&market_id, market)| market.info(market_id))
    }

    pub fn tip(
        &self,
        calling_user: UserId,
        user_to_tip: UserId,
        amount: Balance,
    ) -> Result<Economy<UserId>> {
        ensure!(
            amount.is_sign_positive(),
            "can only send positive amounts of money"
        );
        let mut new_economy = self.clone();
        let caller_money = new_economy.balance_mut(calling_user.clone());
        *caller_money -= amount;
        ensure!(
            !caller_money.is_sign_negative(),
            "you can't afford that in this economy"
        );
        let tipped_user_money = new_economy.balance_mut(user_to_tip.clone());
        *tipped_user_money += amount;
        Ok(new_economy)
    }
}
