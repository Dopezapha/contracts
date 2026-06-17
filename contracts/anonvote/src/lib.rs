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
    contract, contractimpl, contracttype, symbol_short, Address, Env, String,
};

// ── Ballot state types ────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BallotState {
    Active,
    ResultPublished,
}

#[contracttype]
#[derive(Clone)]
pub struct BallotMetadata {
    pub created_at: u64,
    pub admin: Address,
    pub state: BallotState,
}

#[contracttype]
#[derive(Clone)]
pub struct BallotStateSnapshot {
    pub tokens_issued: u32,
    pub votes_cast: u32,
    pub result_hash: Option<String>,
    pub created_at: u64,
    pub admin: Address,
    pub state: BallotState,
}

// ── Storage keys ──────────────────────────────────────────────────────────────

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
    /// Whether a ballot has been created: ballot_id_hash → bool
    BallotExists(String),
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
    pub fn record_ballot(env: Env, caller: Address, ballot_id_hash: String) {
        caller.require_auth();
        Self::require_admin(&env, &caller);

        let key = DataKey::BallotExists(ballot_id_hash.clone());
        if env.storage().persistent().has(&key) {
            panic!("ballot already recorded");
        }
        env.storage().persistent().set(&key, &true);
        env.storage()
            .persistent()
            .set(&DataKey::TokensIssued(ballot_id_hash.clone()), &0u32);
        env.storage()
            .persistent()
            .set(&DataKey::VotesCast(ballot_id_hash.clone()), &0u32);

        let metadata = BallotMetadata {
            created_at: env.ledger().timestamp(),
            admin: caller.clone(),
            state: BallotState::Active,
        };
        env.storage()
            .persistent()
            .set(&DataKey::BallotMetadata(ballot_id_hash.clone()), &metadata);

        env.events().publish(
            (symbol_short!("audit"), symbol_short!("ballot_created")),
            (ballot_id_hash.clone(), metadata.created_at, caller),
        );
    }

    /// Increment the token issued count for a ballot.
    /// Called when a voter token is issued.
    pub fn record_token(env: Env, caller: Address, ballot_id_hash: String) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        Self::require_ballot_exists(&env, &ballot_id_hash);

        let key = DataKey::TokensIssued(ballot_id_hash.clone());
        let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_count = count + 1;
        env.storage().persistent().set(&key, &new_count);

        env.events().publish(
            (symbol_short!("audit"), symbol_short!("token_issued")),
            (ballot_id_hash.clone(), new_count),
        );
    }

    /// Increment the votes cast count for a ballot.
    /// Called when a vote is submitted.
    pub fn record_vote(env: Env, caller: Address, ballot_id_hash: String) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        Self::require_ballot_exists(&env, &ballot_id_hash);

        let key = DataKey::VotesCast(ballot_id_hash.clone());
        let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_count = count + 1;
        env.storage().persistent().set(&key, &new_count);

        env.events().publish(
            (symbol_short!("audit"), symbol_short!("vote_cast")),
            (ballot_id_hash.clone(), new_count),
        );
    }

    /// Record the result publication for a ballot.
    /// result_hash: SHA-256 hex of the tally JSON
    pub fn record_result(
        env: Env,
        caller: Address,
        ballot_id_hash: String,
        result_hash: String,
    ) {
        caller.require_auth();
        Self::require_admin(&env, &caller);
        Self::require_ballot_exists(&env, &ballot_id_hash);

        let key = DataKey::ResultHash(ballot_id_hash.clone());
        if env.storage().persistent().has(&key) {
            panic!("result already recorded");
        }
        env.storage().persistent().set(&key, &result_hash.clone());

        // Update ballot state to ResultPublished
        let metadata_key = DataKey::BallotMetadata(ballot_id_hash.clone());
        let mut metadata: BallotMetadata = env.storage().persistent().get(&metadata_key).unwrap();
        metadata.state = BallotState::ResultPublished;
        env.storage().persistent().set(&metadata_key, &metadata);

        env.events().publish(
            (symbol_short!("audit"), symbol_short!("result_published")),
            (ballot_id_hash.clone(), result_hash),
        );
    }

    // ── Read-only queries ────────────────────────────────────────────────────

    /// Get the number of tokens issued for a ballot.
    /// Returns None if the ballot does not exist.
    pub fn get_tokens_issued(env: Env, ballot_id_hash: String) -> Option<u32> {
        if !env.storage().persistent().has(&DataKey::BallotExists(ballot_id_hash.clone())) {
            return None;
        }
        env.storage()
            .persistent()
            .get(&DataKey::TokensIssued(ballot_id_hash))
    }

    /// Get the number of votes cast for a ballot.
    /// Returns None if the ballot does not exist.
    pub fn get_votes_cast(env: Env, ballot_id_hash: String) -> Option<u32> {
        if !env.storage().persistent().has(&DataKey::BallotExists(ballot_id_hash.clone())) {
            return None;
        }
        env.storage()
            .persistent()
            .get(&DataKey::VotesCast(ballot_id_hash))
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
            .has(&DataKey::BallotExists(ballot_id_hash))
    }

    /// Get ballot metadata (created_at, admin, state).
    /// Returns None if the ballot does not exist.
    pub fn get_ballot_metadata(env: Env, ballot_id_hash: String) -> Option<BallotMetadata> {
        env.storage()
            .persistent()
            .get(&DataKey::BallotMetadata(ballot_id_hash))
    }

    /// Get complete ballot state snapshot (tokens, votes, result, metadata).
    /// Returns None if the ballot does not exist.
    pub fn get_ballot_state(env: Env, ballot_id_hash: String) -> Option<BallotStateSnapshot> {
        if !env.storage().persistent().has(&DataKey::BallotExists(ballot_id_hash.clone())) {
            return None;
        }

        let tokens_issued: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::TokensIssued(ballot_id_hash.clone()))
            .unwrap_or(0);
        let votes_cast: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::VotesCast(ballot_id_hash.clone()))
            .unwrap_or(0);
        let result_hash: Option<String> = env
            .storage()
            .persistent()
            .get(&DataKey::ResultHash(ballot_id_hash.clone()));
        let metadata: BallotMetadata = env
            .storage()
            .persistent()
            .get(&DataKey::BallotMetadata(ballot_id_hash))
            .unwrap();

        Some(BallotStateSnapshot {
            tokens_issued,
            votes_cast,
            result_hash,
            created_at: metadata.created_at,
            admin: metadata.admin,
            state: metadata.state,
        })
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

    fn require_ballot_exists(env: &Env, ballot_id_hash: &String) {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::BallotExists(ballot_id_hash.clone()))
        {
            panic!("ballot not found");
        }
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

    #[test]
    fn test_record_ballot_and_query() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        client.record_ballot(&admin, &ballot_hash);
        assert!(client.ballot_exists(&ballot_hash));
        assert_eq!(client.get_tokens_issued(&ballot_hash), Some(0));
        assert_eq!(client.get_votes_cast(&ballot_hash), Some(0));
    }

    #[test]
    fn test_token_and_vote_counts() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        client.record_ballot(&admin, &ballot_hash);
        client.record_token(&admin, &ballot_hash);
        client.record_token(&admin, &ballot_hash);
        client.record_vote(&admin, &ballot_hash);
        assert_eq!(client.get_tokens_issued(&ballot_hash), Some(2));
        assert_eq!(client.get_votes_cast(&ballot_hash), Some(1));
        assert!(!client.is_consistent(&ballot_hash));
        client.record_vote(&admin, &ballot_hash);
        assert!(client.is_consistent(&ballot_hash));
    }

    #[test]
    fn test_record_result() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        let result_hash = String::from_str(&env, "deadbeef");
        client.record_ballot(&admin, &ballot_hash);
        client.record_result(&admin, &ballot_hash, &result_hash);
        assert_eq!(client.get_result_hash(&ballot_hash), Some(result_hash));
    }

    #[test]
    #[should_panic(expected = "unauthorized")]
    fn test_unauthorized_caller() {
        let (env, client, _admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        let attacker = Address::generate(&env);
        client.record_ballot(&attacker, &ballot_hash);
    }

    #[test]
    fn test_ballot_metadata() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        client.record_ballot(&admin, &ballot_hash);

        let metadata = client.get_ballot_metadata(&ballot_hash).unwrap();
        assert_eq!(metadata.admin, admin);
        assert_eq!(metadata.state, BallotState::Active);
        assert!(metadata.created_at > 0);
    }

    #[test]
    fn test_ballot_state_snapshot() {
        let (env, client, admin) = setup();
        let ballot_hash = String::from_str(&env, "abc123");
        let result_hash = String::from_str(&env, "deadbeef");

        client.record_ballot(&admin, &ballot_hash);
        client.record_token(&admin, &ballot_hash);
        client.record_token(&admin, &ballot_hash);
        client.record_vote(&admin, &ballot_hash);
        client.record_result(&admin, &ballot_hash, &result_hash);

        let state = client.get_ballot_state(&ballot_hash).unwrap();
        assert_eq!(state.tokens_issued, 2);
        assert_eq!(state.votes_cast, 1);
        assert_eq!(state.result_hash, Some(result_hash));
        assert_eq!(state.admin, admin);
        assert_eq!(state.state, BallotState::ResultPublished);
    }

    #[test]
    fn test_nonexistent_ballot() {
        let (env, client, _admin) = setup();
        let ballot_hash = String::from_str(&env, "nonexistent");
        assert_eq!(client.get_tokens_issued(&ballot_hash), None);
        assert_eq!(client.get_votes_cast(&ballot_hash), None);
        assert_eq!(client.get_ballot_metadata(&ballot_hash), None);
        assert_eq!(client.get_ballot_state(&ballot_hash), None);
    }
}
