use anyhow::{bail, ensure, Context, Result};
use im::ordmap::OrdMap;
use poise::ChoiceParameter;
use serde::{Deserialize, Serialize};

use crate::{money::Money, share_quantity::ShareQuantity};

pub type MarketId = u64;

const USER_START_BALANCE: Money = Money(1000.0);
const MARKET_CREATION_COST: Money = Money(50.0);

#[derive(Clone, Serialize, Deserialize)]
pub struct Economy<UserId: Ord + Clone> {
    next_market_id: MarketId,
    user_money: OrdMap<UserId, Money>,
    markets: OrdMap<MarketId, Market<UserId>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Market<UserId: Ord + Clone> {
    pub id: MarketId,
    pub creator: UserId,
    pub question: String,
    pub description: String,
    y: ShareQuantity,
    n: ShareQuantity,
    pub num_user_shares: OrdMap<UserId, UserShareBalance>,
    pub close_timestamp: Option<i64>,
}

pub struct Portfolio {
    pub cash: Money,
    pub market_positions: Vec<(String, UserShareBalance)>,
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, ChoiceParameter)]
pub enum ShareKind {
    #[name = "YES"]
    Yes,
    #[name = "NO"]
    No,
}

#[derive(Clone, Serialize, Deserialize, derive_more::Display)]
#[display(fmt = "{quantity} {kind}")]
pub struct UserShareBalance {
    pub kind: ShareKind,
    pub quantity: ShareQuantity,
}

impl<UserId: Ord + Clone> Market<UserId> {
    fn new(
        id: MarketId,
        creator: UserId,
        question: String,
        description: String,
        close_timestamp: Option<i64>,
    ) -> Self {
        Market {
            id,
            creator,
            question,
            description,
            y: ShareQuantity(MARKET_CREATION_COST.0),
            n: ShareQuantity(MARKET_CREATION_COST.0),
            num_user_shares: OrdMap::new(),
            close_timestamp,
        }
    }

    pub fn probability(&self) -> u8 {
        let p = self.n / (self.y + self.n);
        (p.0 * 100.0) as u8
    }

    pub fn is_open(&self) -> bool {
        match self.close_timestamp {
            None => true,
            Some(close_timestamp) => chrono::Local::now().timestamp() < close_timestamp,
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

    pub fn market(&self, market_id: MarketId) -> Result<&Market<UserId>> {
        self.markets
            .get(&market_id)
            .context("failed getting market name because market ID does not exist")
    }

    pub fn balances(&self) -> Vec<(UserId, Money)> {
        let mut ret = self
            .user_money
            .iter()
            .map(|(user_id, balance)| (user_id.clone(), *balance))
            .collect::<Vec<(UserId, Money)>>();
        ret.sort_by(|(_, a), (_, b)| b.partial_cmp(a).expect("failed comparing balances"));
        ret
    }

    pub fn balance(&self, user: UserId) -> Money {
        *self.user_money.get(&user).unwrap_or(&USER_START_BALANCE)
    }

    fn balance_mut(&mut self, user: UserId) -> &mut Money {
        self.user_money.entry(user).or_insert(USER_START_BALANCE)
    }

    pub fn portfolio(&self, user: UserId) -> Portfolio {
        Portfolio {
            cash: self.balance(user.clone()),
            market_positions: self
                .markets
                .values()
                .filter_map(|market| {
                    market
                        .num_user_shares
                        .get(&user)
                        .map(|user_shares| (market.question.clone(), user_shares.clone()))
                })
                .collect(),
        }
    }

    pub fn create_market(
        &self,
        calling_user: UserId,
        question: String,
        description: String,
        close_timestamp: Option<i64>,
    ) -> Result<(Economy<UserId>, MarketId)> {
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
            !user_money.0.is_sign_negative(),
            "can't afford market creation cost"
        );

        // Create market
        let market = Market::new(
            market_id,
            calling_user,
            question,
            description,
            close_timestamp,
        );
        ensure!(
            new_economy.markets.insert(market_id, market).is_none(),
            "somehow, market with this id exists already"
        );

        Ok((new_economy, market_id))
    }

    pub fn resolve_market(
        &self,
        calling_user: UserId,
        market_id: MarketId,
        outcome: ShareKind,
    ) -> Result<(Economy<UserId>, Market<UserId>)> {
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
                *user_money += Money(share_balance.quantity.0)
            }
        }

        let caller_money = new_economy.balance_mut(calling_user);
        match outcome {
            ShareKind::No => *caller_money += Money(market.n.0),
            ShareKind::Yes => *caller_money += Money(market.y.0),
        }

        let market = new_economy.markets.remove(&market_id).context("market does not exist, after we already accessed it?? this definitely shouldn't happen")?;

        Ok((new_economy, market))
    }

    pub fn sell(
        &self,
        calling_user: UserId,
        market_id: MarketId,
        sell_amount: Option<ShareQuantity>,
    ) -> Result<(Economy<UserId>, UserShareBalance, Money)> {
        let mut new_economy = self.clone();
        let market = new_economy
            .markets
            .get_mut(&market_id)
            .context("market does not exist")?;
        ensure!(market.is_open(), "this market closed");
        let product = market.y.0 * market.n.0;
        let shares_sold = match sell_amount {
            None => {
                let user_shares = market
                    .num_user_shares
                    .get(&calling_user)
                    .context("you have no shares to sell")?
                    .clone();
                market.num_user_shares.remove(&calling_user);
                user_shares
            }
            Some(num_shares_to_sell) => {
                let user_shares = market
                    .num_user_shares
                    .get_mut(&calling_user)
                    .context("you have no shares to sell")?;
                let num_shares = &mut user_shares.quantity;
                ensure!(
                    num_shares_to_sell.0.is_sign_positive(),
                    "must sell a positive number of shares"
                );
                *num_shares -= num_shares_to_sell;
                ensure!(
                    !num_shares.0.is_sign_negative(),
                    "you are trying to sell more shares than you have"
                );
                UserShareBalance {
                    kind: user_shares.kind,
                    quantity: num_shares_to_sell,
                }
            }
        };
        let num_market_shares = match shares_sold.kind {
            ShareKind::No => &mut market.n,
            ShareKind::Yes => &mut market.y,
        };
        *num_market_shares += shares_sold.quantity;
        let y = market.y.0;
        let n = market.n.0;
        let k = product;
        let sale_price = (y + n - ((y + n).powf(2.0) + 4.0 * (k - n * y)).sqrt()) / 2.0;
        market.n -= ShareQuantity(sale_price);
        ensure!(
            !market.n.0.is_sign_negative(),
            "underflow balancing market NO shares"
        );
        market.y -= ShareQuantity(sale_price);
        ensure!(
            !market.y.0.is_sign_negative(),
            "underflow balancing market YES shares"
        );
        let user_money = new_economy.balance_mut(calling_user);
        *user_money += Money(sale_price);
        Ok((new_economy, shares_sold, Money(sale_price)))
    }

    pub fn buy(
        &self,
        calling_user: UserId,
        market_id: MarketId,
        purchase_price: Money,
        share_kind: ShareKind,
    ) -> Result<(Economy<UserId>, ShareQuantity)> {
        ensure!(
            purchase_price.0.is_sign_positive(),
            "must buy with a positive amount of money"
        );
        let mut new_economy = self.clone();
        let user_money = new_economy.balance_mut(calling_user.clone());
        *user_money -= purchase_price;
        ensure!(
            !user_money.0.is_sign_negative(),
            "you can't afford that in this economy"
        );
        let market = new_economy
            .markets
            .get_mut(&market_id)
            .context("market does not exist")?;
        ensure!(market.is_open(), "this market closed");
        let product = market.y * market.n;
        let num_new_shares = ShareQuantity(purchase_price.0);
        market.n += num_new_shares;
        market.y += num_new_shares;
        let n = market.n;
        let y = market.y;
        let k = product;
        let bought_shares = match share_kind {
            ShareKind::No => {
                let bought_shares = (n * y - k) / y;
                market.n -= bought_shares;
                ensure!(
                    !market.n.0.is_sign_negative(),
                    "underflow subtracting NO shares for user"
                );
                bought_shares
            }
            ShareKind::Yes => {
                let bought_shares = (n * y - k) / n;
                market.y -= bought_shares;
                ensure!(
                    !market.y.0.is_sign_negative(),
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

    pub fn list_markets(&self) -> impl Iterator<Item = &Market<UserId>> + '_ {
        self.markets.values()
    }

    pub fn tip(
        &self,
        calling_user: UserId,
        user_to_tip: UserId,
        amount: Money,
    ) -> Result<Economy<UserId>> {
        ensure!(
            amount.0.is_sign_positive(),
            "can only send positive amounts of money"
        );
        let mut new_economy = self.clone();
        let caller_money = new_economy.balance_mut(calling_user);
        *caller_money -= amount;
        ensure!(
            !caller_money.0.is_sign_negative(),
            "you can't afford that in this economy"
        );
        let tipped_user_money = new_economy.balance_mut(user_to_tip);
        *tipped_user_money += amount;
        Ok(new_economy)
    }
}
