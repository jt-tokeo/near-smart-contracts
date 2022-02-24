/*!
STAKING implementation 
NOTES:
  TCC Staking: with TCC interest rates as high as 1% APR :)
*/
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{assert_one_yocto,env, near_bindgen,setup_alloc, AccountId,PanicOnDefault,PromiseResult,PromiseOrValue};
use near_sdk::json_types::{ValidAccountId, U128};
use near_sdk::collections::LookupMap;
use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::ext_contract;

const CALLBACK_GAS: u64 = 5_000_000_000_000;
const TRANSFER_GAS: u64 = 25_000_000_000_000 + CALLBACK_GAS;

setup_alloc!();

#[ext_contract(ext_token_contract)]
trait ExternalToken{
    fn ft_transfer(&mut self,receiver_id :AccountId,amount:U128,memo :Option<String>);
}

#[ext_contract(ext_self)]
trait TransferResolver{
    fn callback_withdraw_stake(&mut self,account_id:AccountId,amount:u128);
    fn callback_withdraw_interests(&mut self,account_id:AccountId);
}

trait TransferResolver{
    fn callback_withdraw_stake(&mut self,account_id:AccountId,amount:u128);
    fn callback_withdraw_interests(&mut self,account_id:AccountId);
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct UserStake{
    amount : u128,
    interests_before : u128,
    date : u64
}

impl UserStake{
    pub fn new(amount:u128)->Self{
        let mut stake = UserStake{
            amount : 0,
            interests_before : 0,
            date : env::block_timestamp()/1_000_000_000,
        };
        stake.amount = amount;
        stake
    }
    pub fn calc_interests(&self,rate:u128)->u128{
        let time : u64 = env::block_timestamp()/1_000_000_000 - self.date;
        let mut interests : u128 = self.amount;
        interests *= u128::from(time);
        interests *= rate;
        interests /= u128::pow(10,8);
        interests /= 31557600;
        interests += self.interests_before;
        interests

    }
    pub fn stock_interests_before(&mut self,rate:u128){
        self.interests_before = self.calc_interests(rate);
        self.date = env::block_timestamp()/1_000_000_000;

    }
    pub fn stake(&mut self,amount : u128,rate:u128){
        self.stock_interests_before(rate);
        self.amount += amount;
    }
    pub fn unstake(&mut self,amount : u128,rate:u128){
        self.stock_interests_before(rate);
        self.amount -= amount;
    }
    pub fn reset_interests(&mut self){
        self.interests_before = 0;
        self.date = env::block_timestamp()/1_000_000_000;
    }
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Staking {
    stakes: LookupMap<String, UserStake>,
    interest_rate : u128,
    token_account_id : AccountId,
}

#[near_bindgen]
impl Staking {

    #[init]
    pub fn new(
        interest_rate : U128,
        token_account_id : AccountId
    )->Self
    {
        assert!(!env::state_exists(), "Already initialized");
        let this = Self{
            stakes: LookupMap::new(b"a".to_vec()),
            interest_rate: interest_rate.0,
            token_account_id,
        };
        this
    }
    pub fn staking_amount_of(&self,account_id:ValidAccountId)->U128{
        let stakeresult = self.stakes.get(&account_id.to_string());
        match stakeresult{
            Some(stake) => stake.amount.into(),
            None => U128(0)
        }
    }
    pub fn staking_interests_of(&self,account_id:ValidAccountId)->U128{
        let stakeresult = self.stakes.get(&account_id.to_string());
        match stakeresult{
            Some(stake) => stake.calc_interests(self.interest_rate).into(),
            None => U128(0)
        }
    }
    #[payable]
    pub fn staking_withdraw_stake(&mut self,amount:U128){
        assert_one_yocto();
        assert!(
            env::prepaid_gas() > TRANSFER_GAS + CALLBACK_GAS,
            "More gas is required"
        );
        let stakeresult = self.stakes.get(&env::predecessor_account_id());
        let stake_amount : u128 = match stakeresult{
            Some(stake) => stake.amount,
            None => env::panic(b"you have no stake")
        };
        assert!(stake_amount >= amount.0,"you don't have enough stake");
        ext_token_contract::ft_transfer(env::predecessor_account_id(),amount,None,&self.token_account_id,1, TRANSFER_GAS).then(
            ext_self::callback_withdraw_stake(env::predecessor_account_id(),amount.0,&env::current_account_id(),0,CALLBACK_GAS)
        );
    }
    #[payable]
    pub fn staking_withdraw_interests(&mut self){
        assert_one_yocto();
        assert!(
            env::prepaid_gas() > TRANSFER_GAS + CALLBACK_GAS,
            "More gas is required"
        );
        let stakeresult = self.stakes.get(&env::predecessor_account_id());
        let amount : u128 = match stakeresult{
            Some(stake) => stake.calc_interests(self.interest_rate),
            None => env::panic(b"you have no interests")
        };
        ext_token_contract::ft_transfer(env::predecessor_account_id(),U128(amount),None,&self.token_account_id,1, TRANSFER_GAS).then(
            ext_self::callback_withdraw_interests(env::predecessor_account_id(),&env::current_account_id(),0,CALLBACK_GAS)
        );
    }
}
#[near_bindgen]
impl FungibleTokenReceiver for Staking{
    fn ft_on_transfer(&mut self, sender_id: ValidAccountId, amount: U128, msg: String)->PromiseOrValue<U128>{
        assert!(
            env::predecessor_account_id() == self.token_account_id,
            "Only supports the one fungible token contract"
        );
        assert!(
            msg == "stake",
            "Only stake as message"
        );
        let oldstake = self.stakes.get(&sender_id.to_string());
        let mut newstake: UserStake= match oldstake{
            Some(stake) => stake,
            None => UserStake::new(0)
        };

        newstake.stake(amount.0, self.interest_rate);
        self.stakes.insert(&sender_id.to_string(),&newstake);


        PromiseOrValue::Value(U128(0))
    }
}

#[near_bindgen]
impl TransferResolver for Staking{
    fn callback_withdraw_stake(&mut self,account_id:AccountId,amount:u128){
        match env::promise_result(0){
            PromiseResult::NotReady => env::panic(b"promise not ready"),
            PromiseResult::Failed => env::panic(b"Transfer transaction failed"),
            PromiseResult::Successful(_) => {
                let mut stake = self.stakes.get(&account_id).unwrap();
                stake.unstake(amount, self.interest_rate);
                self.stakes.insert(&account_id,&stake);
            }
        }
    }
    fn callback_withdraw_interests(&mut self,account_id:AccountId){
        match env::promise_result(0){
            PromiseResult::NotReady => env::panic(b"promise not ready"),
            PromiseResult::Failed => env::panic(b"Transfer transaction failed"),
            PromiseResult::Successful(_) => {
                let mut stake = self.stakes.get(&account_id).unwrap();
                stake.reset_interests();
                self.stakes.insert(&account_id,&stake);
            }
        }
    }
        
}
