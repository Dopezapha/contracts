# AnonVote Contracts

**Soroban smart contracts for on-chain audit and verification of AnonVote ballots.**

This repo contains all Stellar/Soroban contract code for the AnonVote ecosystem. Contracts provide on-chain queryable state that complements the off-chain privacy engine — giving anyone the ability to verify ballot integrity directly on the Stellar ledger without trusting AnonVote's servers.

[![Rust](https://img.shields.io/badge/Rust-1.78+-orange)](https://www.rust-lang.org/)
[![Soroban SDK](https://img.shields.io/badge/soroban--sdk-21.0.0-blueviolet)](https://github.com/stellar/rs-soroban-sdk)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## Role in the ecosystem

| Repo                                                      | Relationship                                                    |
| --------------------------------------------------------- | --------------------------------------------------------------- |
| [AnonVote/core](https://github.com/AnonVote/core)         | Backend calls `sorobanService.ts` which invokes these contracts |
| [AnonVote/js](https://github.com/AnonVote/js)             | No dependency — contracts are Rust, not JS                      |
| [AnonVote/protocol](https://github.com/AnonVote/protocol) | Whitepaper references contract audit model                      |

---

## What's in this repo

### `contracts/anonvote/` — AnonVote Audit Contract

The primary Soroban contract. Records immutable audit events on-chain with public read access.

| Function                                     | Description                                                          |
| -------------------------------------------- | -------------------------------------------------------------------- |
| `initialize(admin)`                          | One-time setup after deployment. Sets the admin address.             |
| `record_ballot(ballot_id_hash)`              | Register a ballot on-chain. Input is SHA-256 hex of the ballot UUID. |
| `record_token(ballot_id_hash)`               | Increment the token-issued count for a ballot.                       |
| `record_vote(ballot_id_hash)`                | Increment the vote-cast count for a ballot.                          |
| `configure_approval_threshold(approvers, m, n)` | Configure the M-of-N governance approver set (admin only). |
| `record_result(...)`, `rotate_admin(...)`, `pause_contract(...)`, `schedule_upgrade(...)` | Create a pending critical operation and return its operation ID. |
| `create_operation(operation)`                 | Generic proposal entrypoint for a typed critical operation. |
| `approve_operation(operation_id, approver)`  | Approve a pending operation; the threshold-reaching approval executes it. |
| `cancel_operation(operation_id)`              | Cancel a pending critical operation (admin only). |
| `get_operation(operation_id)`                 | Read operation details, status, expiry, and approval count. |
| `get_tokens_issued(ballot_id_hash)`          | Read token count (view call).                                        |
| `get_votes_cast(ballot_id_hash)`             | Read vote count (view call).                                         |
| `get_result_hash(ballot_id_hash)`            | Read result hash (view call).                                        |
| `ballot_exists(ballot_id_hash)`              | Check if a ballot has been recorded on-chain.                        |
| `is_consistent(ballot_id_hash)`              | Returns `true` if `tokens_issued == votes_cast`.                     |

**Privacy guarantees:**

- No voter identifiers stored
- No token values stored
- No vote content stored
- Only counts and hashes — same privacy model as the off-chain system

**Governance guarantees:**

- Critical operations require distinct approvals from a configurable M-of-N address set.
- Pending operations expire after seven days and can be cancelled before execution.
- Approval, creation, cancellation, and execution events provide an on-chain audit trail.
- Upgrade scheduling remains subject to its existing 48-hour execution timelock after approval.

### `service/sorobanService.ts` — TypeScript Service Stub

A fully-typed TypeScript wrapper around the Soroban contract, ready to wire into [AnonVote/core](https://github.com/AnonVote/core). Uses `stellar-sdk` v12 with correct RPC, simulation, and assembly APIs.

---

## Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add WASM target
rustup target add wasm32-unknown-unknown

# Install Stellar CLI
cargo install --locked stellar-cli --features opt
```

---

## Build

```bash
cd contracts/anonvote
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/anonvote.wasm`

---

## Test

```bash
cd contracts/anonvote
cargo test
```

All tests run in the Soroban native test environment — no network required.

---

## Deploy to Testnet

```bash
# Deploy the contract WASM
stellar contract deploy \
  --wasm contracts/anonvote/target/wasm32-unknown-unknown/release/anonvote.wasm \
  --source <YOUR_SECRET_KEY> \
  --network testnet

# Output: CONTRACT_ID (e.g. CABC123...)

# Initialize with your admin public key
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <YOUR_SECRET_KEY> \
  --network testnet \
  -- initialize \
  --admin <YOUR_PUBLIC_KEY>
```

Then add to `core`'s `backend/.env`:

```env
SOROBAN_CONTRACT_ID=<CONTRACT_ID>
```

---

## Wiring into core

Once deployed, the `sorobanService.ts` helpers are called from these locations in [AnonVote/core](https://github.com/AnonVote/core):

| Core file                     | Contract call                                   |
| ----------------------------- | ----------------------------------------------- |
| `services/ballotEngine.ts`    | `sorobanRecordBallot(ballotIdHash)`             |
| `services/identityManager.ts` | `sorobanRecordToken(ballotIdHash)`              |
| `services/privacyEngine.ts`   | `sorobanRecordVote(ballotIdHash)`               |
| `services/resultEngine.ts`    | `sorobanRecordResult(ballotIdHash, resultHash)` |

The `ballot_id_hash` argument is `hashIdentifier(ballotId)` from `@anonvote/crypto`.

---

## Repository structure

```
contracts/
├── contracts/
│   └── anonvote/
│       ├── src/
│       │   └── lib.rs       # Soroban contract implementation
│       ├── Cargo.toml
│       └── README.md
├── service/
│   └── sorobanService.ts    # TypeScript service stub for core
└── README.md
```

---

## License

[MIT](LICENSE)
