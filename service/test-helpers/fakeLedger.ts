/**
 * In-memory stand-in for the deployed AnonVote contract, used only to drive
 * the integration test's mocked RPC responses.
 *
 * IMPORTANT CAVEAT: this mirrors the *return values* of the methods in
 * contracts/anonvote/src/lib.rs (record_ballot, record_token, ... ,
 * is_consistent) closely enough to exercise sorobanService.ts's control flow
 * — error mapping, idempotency, retries. It does NOT execute real WASM, does
 * NOT enforce require_auth(), and applies state during the simulate step for
 * simplicity (real Soroban applies state on tx confirmation, not simulation).
 * Contract correctness itself stays the responsibility of the Rust tests in
 * lib.rs — this fake exists purely so the TS service can be integration-
 * tested without a live network, per the issue's acceptance criteria.
 */

import * as crypto from "crypto";

interface MerkleProof {
  vote_hash: Buffer;
  path: Buffer[];
  index: number;
}

type FakeBallot = {
  admin: string;
  tokensIssued: number;
  votesCast: number;
  resultHash: string | null;
  state: "Active" | "ResultPublished" | "Archived";
};

export type LedgerOutcome =
  | { ok: true; value?: unknown }
  | { ok: false; contractErrorCode: number };

// Mirrors ContractError in lib.rs
const ContractErrorCode = {
  BallotNotFound: 4,
  BallotAlreadyExists: 5,
  ResultAlreadyPublished: 6,
  InvalidStateTransition: 12,
};

export class FakeLedger {
  private ballots = new Map<string, FakeBallot>();

  call(method: string, args: { value: unknown }[]): LedgerOutcome {
    const get = (i: number) => args[i]?.value;

    switch (method) {
      case "record_ballot": {
        const caller = get(0) as string;
        const ballotIdHash = get(1) as string;
        const existing = this.ballots.get(ballotIdHash);
        if (existing) {
          if (existing.admin === caller) return { ok: true };
          return { ok: false, contractErrorCode: ContractErrorCode.BallotAlreadyExists };
        }
        this.ballots.set(ballotIdHash, {
          admin: caller,
          tokensIssued: 0,
          votesCast: 0,
          resultHash: null,
          state: "Active",
        });
        return { ok: true };
      }

      case "record_token": {
        const ballot = this.ballots.get(get(1) as string);
        if (!ballot) return { ok: false, contractErrorCode: ContractErrorCode.BallotNotFound };
        ballot.tokensIssued++;
        return { ok: true };
      }

      case "record_vote": {
        const ballot = this.ballots.get(get(1) as string);
        if (!ballot) return { ok: false, contractErrorCode: ContractErrorCode.BallotNotFound };
        ballot.votesCast++;
        return { ok: true };
      }

      case "record_result": {
        const ballot = this.ballots.get(get(1) as string);
        if (!ballot) return { ok: false, contractErrorCode: ContractErrorCode.BallotNotFound };
        const resultHash = get(2) as string;
        if (ballot.resultHash !== null && ballot.resultHash !== resultHash) {
          return { ok: false, contractErrorCode: ContractErrorCode.ResultAlreadyPublished };
        }
        ballot.resultHash = resultHash;
        ballot.state = "ResultPublished";
        return { ok: true };
      }

      case "transition_ballot_state": {
        const ballot = this.ballots.get(get(1) as string);
        if (!ballot) return { ok: false, contractErrorCode: ContractErrorCode.BallotNotFound };
        const newState = get(2) as string;
        const valid =
          (ballot.state === "Active" && newState === "ResultPublished") ||
          (ballot.state === "ResultPublished" && newState === "Archived");
        if (!valid) {
          return { ok: false, contractErrorCode: ContractErrorCode.InvalidStateTransition };
        }
        ballot.state = newState as "Active" | "ResultPublished" | "Archived";
        return { ok: true };
      }

      case "get_tokens_issued": {
        const ballot = this.ballots.get(get(0) as string);
        // None (ballot missing) -> value: undefined, matching Option<u32>::None
        return { ok: true, value: ballot ? ballot.tokensIssued : undefined };
      }

      case "get_votes_cast": {
        const ballot = this.ballots.get(get(0) as string);
        return { ok: true, value: ballot ? ballot.votesCast : undefined };
      }

      case "get_result_hash": {
        const ballot = this.ballots.get(get(0) as string);
        return { ok: true, value: ballot?.resultHash ?? undefined };
      }

      case "result_exists": {
        const ballot = this.ballots.get(get(0) as string);
        return { ok: true, value: ballot !== undefined && ballot.resultHash !== null };
      }

      case "is_consistent": {
        const ballot = this.ballots.get(get(0) as string);
        if (!ballot) return { ok: true, value: true }; // 0 == 0, matches lib.rs default
        return { ok: true, value: ballot.tokensIssued === ballot.votesCast };
      }

      case "get_audit_report": {
        const ballotIdHash = get(0) as string;
        const ballot = this.ballots.get(ballotIdHash);
        if (!ballot) return { ok: true, value: undefined }; // matches Option::None
        return {
          ok: true,
          value: {
            admin: ballot.admin,
            created_at: 1718880000,
            expiration_time: 0,
            is_consistent: ballot.tokensIssued === ballot.votesCast,
            result_hash: ballot.resultHash,
            state: ballot.state,
            tokens_issued: ballot.tokensIssued,
            votes_cast: ballot.votesCast,
          },
        };
      }

      case "verify_result_proof": {
        const ballotIdHash = get(0) as string;
        const ballot = this.ballots.get(ballotIdHash);
        if (!ballot) return { ok: false, contractErrorCode: ContractErrorCode.BallotNotFound };
        if (ballot.resultHash === null) {
          return { ok: false, contractErrorCode: ContractErrorCode.BallotNotFound };
        }

        const proof = get(1) as MerkleProof;
        const resultHashParam = get(2) as string;

        let currentHash = proof.vote_hash;
        let idx = proof.index;

        for (const sibling of proof.path) {
          let data: Buffer;
          if (idx % 2 === 0) {
            data = Buffer.concat([currentHash, sibling]);
          } else {
            data = Buffer.concat([sibling, currentHash]);
          }
          currentHash = crypto.createHash("sha256").update(data).digest();
          idx = Math.floor(idx / 2);
        }

        const computedRootHex = currentHash.toString("hex");

        if (computedRootHex !== resultHashParam) {
          return { ok: true, value: false };
        }
        if (ballot.resultHash !== resultHashParam) {
          return { ok: true, value: false };
        }

        return { ok: true, value: true };
      }

      default:
        throw new Error(`FakeLedger: unhandled method "${method}"`);
    }
  }

  reset() {
    this.ballots.clear();
  }
}