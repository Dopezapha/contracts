//! AnonVote Soroban Smart Contract
//!
//! Records immutable audit events on the Stellar blockchain.
//! Complements the manageData approach with on-chain queryable state.
//!
//! # What this contract does
//! - Records ballot creation events with a ballot ID hash
//! - Records token issuance counts per ballot (no voter identity)
//! - Records vote cast counts per ballot (no vote content)
//! - Records result publication with a tally hash
//! - Allows public verification of event counts on-chain
//!
//! # Privacy guarantees
//! - No voter identifiers stored
//! - No token values stored
//! - No vote content stored
//! - Only counts and hashes — same privacy model as the off-chain system

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, String,
};

// ── Storage keys ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub struct BallotLimits {
    pub max_tokens: u32,
    pub max_votes: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct BallotMetadata {
    pub limits: BallotLimits,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    LimitExceeded = 1,
    BallotNotFound = 2,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Admin address — only admin can record events
    Admin,
    /// Token issued count for a ballot: ballot_id_hash → u32
    TokensIssued(String),
    /// Votes cast count for a ballot: ballot_id_hash → u32
    VotesCast(String),
    /// Result hash for a ballot: ballot_id_hash → String
    ResultHash(String),
    /// Ballot metadata: ballot_id_hash → BallotMetadata
    BallotMetadata(String),
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct AnonVoteContract;

#[contractimpl]
impl AnonVoteContract {
    /// Initialize the contract with an admin address.
    /// Must be called once after deployment.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Record a ballot creation event.
    /// ballot_id_hash: SHA-256 hex of the ballot UUID
    pub fn record_ballot(
        env: Env,
        caller: Address,
        ballot_id_hash: String,
        limits: BallotLimits,
    ) -> Result<(), Error> {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::BallotMetadata(ballot_id_hash.clone());
        if env.storage().persistent().has(&key) {
            panic!("ballot already recorded");
        }
        env.storage()
            .persistent()
            .set(&key, &BallotMetadata { limits });
        env.storage()
            .persistent()
            .set(&DataKey::TokensIssued(ballot_id_hash.clone()), &0u32);
        env.storage()
            .persistent()
            .set(&DataKey::VotesCast(ballot_id_hash), &0u32);

        env.events()
            .publish((symbol_short!("ballot"),), (symbol_short!("created"),));
        Ok(())
    }

    /// Increment the token issued count for a ballot.
    /// Called when a voter token is issued.
    pub fn record_token(env: Env, caller: Address, ballot_id_hash: String) -> Result<(), Error> {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        let metadata = Self::require_ballot_metadata(&env, &ballot_id_hash)?;

        let key = DataKey::TokensIssued(ballot_id_hash);
        let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        if count >= metadata.limits.max_tokens {
            env.events().publish(
                (symbol_short!("limit"), symbol_short!("token")),
                (symbol_short!("current"), count),
            );
            return Err(Error::LimitExceeded);
        }
        env.storage().persistent().set(&key, &(count + 1));

        env.events()
            .publish((symbol_short!("token"),), (symbol_short!("issued"),));
        Ok(())
    }

    /// Increment the votes cast count for a ballot.
    /// Called when a vote is submitted.
    pub fn record_vote(env: Env, caller: Address, ballot_id_hash: String) -> Result<(), Error> {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        let metadata = Self::require_ballot_metadata(&env, &ballot_id_hash)?;

        let key = DataKey::VotesCast(ballot_id_hash);
        let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        if count >= metadata.limits.max_votes {
            env.events().publish(
                (symbol_short!("limit"), symbol_short!("vote")),
                (symbol_short!("current"), count),
            );
            return Err(Error::LimitExceeded);
        }
        env.storage().persistent().set(&key, &(count + 1));

        env.events()
            .publish((symbol_short!("vote"),), (symbol_short!("cast"),));
        Ok(())
    }

    /// Record the result publication for a ballot.
    /// result_hash: SHA-256 hex of the tally JSON
    pub fn record_result(
        env: Env,
        caller: Address,
        ballot_id_hash: String,
        result_hash: String,
    ) -> Result<(), Error> {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        Self::require_ballot_metadata(&env, &ballot_id_hash)?;

        let key = DataKey::ResultHash(ballot_id_hash);
        if env.storage().persistent().has(&key) {
            panic!("result already recorded");
        }
        env.storage().persistent().set(&key, &result_hash);

        env.events()
            .publish((symbol_short!("result"),), (symbol_short!("published"),));
        Ok(())
    }

    // ── Read-only queries ────────────────────────────────────────────────────

    /// Get the number of tokens issued for a ballot.
    pub fn get_tokens_issued(env: Env, ballot_id_hash: String) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::TokensIssued(ballot_id_hash))
            .unwrap_or(0)
    }

    /// Get the number of votes cast for a ballot.
    pub fn get_votes_cast(env: Env, ballot_id_hash: String) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::VotesCast(ballot_id_hash))
            .unwrap_or(0)
    }

    /// Get the result hash for a ballot (None if not yet published).
    pub fn get_result_hash(env: Env, ballot_id_hash: String) -> Option<String> {
        env.storage()
            .persistent()
            .get(&DataKey::ResultHash(ballot_id_hash))
    }

    /// Check if a ballot has been recorded on-chain.
    pub fn ballot_exists(env: Env, ballot_id_hash: String) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::BallotMetadata(ballot_id_hash))
    }

    /// Verify consistency: returns true if tokens_issued == votes_cast.
    pub fn is_consistent(env: Env, ballot_id_hash: String) -> bool {
        let tokens: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::TokensIssued(ballot_id_hash.clone()))
            .unwrap_or(0);
        let votes: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::VotesCast(ballot_id_hash))
            .unwrap_or(0);
        tokens == votes
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        if *caller != admin {
            panic!("unauthorized");
        }
    }

    fn require_ballot_metadata(
        env: &Env,
        ballot_id_hash: &String,
    ) -> Result<BallotMetadata, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::BallotMetadata(ballot_id_hash.clone()))
            .ok_or(Error::BallotNotFound)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env, String};

    fn setup() -> (Env, AnonVoteContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, AnonVoteContract);
        let client = AnonVoteContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin)
    }

    fn limits(max_tokens: u32, max_votes: u32) -> BallotLimits {
        BallotLimits {
            max_tokens,
            max_votes,
        }
    }

    #[test]
    fn test_record_ballot_and_query() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        client.record_ballot(&admin, &ballot_hash, &limits(10, 10));
        assert!(client.ballot_exists(&ballot_hash));
        assert_eq!(client.get_tokens_issued(&ballot_hash), 0);
        assert_eq!(client.get_votes_cast(&ballot_hash), 0);
    }

    #[test]
    fn test_token_and_vote_counts() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        client.record_ballot(&admin, &ballot_hash, &limits(10, 10));
        client.record_token(&admin, &ballot_hash);
        client.record_token(&admin, &ballot_hash);
        client.record_vote(&admin, &ballot_hash);
        assert_eq!(client.get_tokens_issued(&ballot_hash), 2);
        assert_eq!(client.get_votes_cast(&ballot_hash), 1);
        assert!(!client.is_consistent(&ballot_hash));
        client.record_vote(&admin, &ballot_hash);
        assert!(client.is_consistent(&ballot_hash));
    }

    #[test]
    fn test_record_result() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        let result_hash = String::from_str(&env, "deadbeef");
        client.record_ballot(&admin, &ballot_hash, &limits(10, 10));
        client.record_result(&admin, &ballot_hash, &result_hash);
        assert_eq!(client.get_result_hash(&ballot_hash), Some(result_hash));
    }

    #[test]
    fn test_limits_are_enforced_correctly() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "limited");
        client.record_ballot(&admin, &ballot_hash, &limits(2, 1));

        assert_eq!(client.try_record_token(&admin, &ballot_hash), Ok(Ok(())));
        assert_eq!(client.try_record_token(&admin, &ballot_hash), Ok(Ok(())));
        assert_eq!(
            client.try_record_token(&admin, &ballot_hash),
            Err(Ok(Error::LimitExceeded))
        );
        assert_eq!(client.get_tokens_issued(&ballot_hash), 2);

        assert_eq!(client.try_record_vote(&admin, &ballot_hash), Ok(Ok(())));
        assert_eq!(
            client.try_record_vote(&admin, &ballot_hash),
            Err(Ok(Error::LimitExceeded))
        );
        assert_eq!(client.get_votes_cast(&ballot_hash), 1);
    }

    #[test]
    fn test_zero_limit_blocks_all_operations() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "zero");
        client.record_ballot(&admin, &ballot_hash, &limits(0, 0));

        assert_eq!(
            client.try_record_token(&admin, &ballot_hash),
            Err(Ok(Error::LimitExceeded))
        );
        assert_eq!(
            client.try_record_vote(&admin, &ballot_hash),
            Err(Ok(Error::LimitExceeded))
        );
        assert_eq!(client.get_tokens_issued(&ballot_hash), 0);
        assert_eq!(client.get_votes_cast(&ballot_hash), 0);
    }

    #[test]
    #[should_panic(expected = "unauthorized")]
    fn test_unauthorized_caller() {
        let (env, client, _admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        let attacker = Address::generate(&env);
        client.record_ballot(&attacker, &ballot_hash, &limits(10, 10));
    }
}
