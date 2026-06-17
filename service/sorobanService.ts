/**
 * AnonVote Soroban Service
 *
 * TypeScript service for invoking the AnonVote Soroban smart contract from
 * the AnonVote/core backend.
 *
 * STATUS: Contract written (contracts/anonvote/src/lib.rs) — needs deployment.
 * The manageData-based stellarService is the active blockchain layer.
 * This service is ready to wire once the Soroban contract is deployed.
 *
 * TO ACTIVATE:
 * 1. Build the contract:
 *      cd contracts/anonvote && cargo build --target wasm32-unknown-unknown --release
 * 2. Deploy to testnet:
 *      stellar contract deploy --wasm target/wasm32-unknown-unknown/release/anonvote.wasm --network testnet
 * 3. Initialize:
 *      stellar contract invoke --id <ID> -- initialize --admin <PUBLIC_KEY>
 * 4. Set SOROBAN_CONTRACT_ID=<ID> in backend/.env
 * 5. Call the helpers below from ballotEngine, identityManager, privacyEngine, resultEngine
 *
 * SDK USAGE (stellar-sdk v12):
 * - RPC server:     new StellarSdk.SorobanRpc.Server(rpcUrl)
 * - Simulate tx:    server.simulateTransaction(tx)
 * - Assemble tx:    StellarSdk.SorobanRpc.assembleTransaction(tx, simulation)
 * - Submit tx:      server.sendTransaction(tx)
 * - Convert values: StellarSdk.nativeToScVal(value, { type }) / scValToNative(scVal)
 */

import * as StellarSdk from "stellar-sdk";

const SOROBAN_RPC_TESTNET = "https://soroban-testnet.stellar.org";
const SOROBAN_RPC_MAINNET = "https://rpc.stellar.org";

export interface SorobanConfig {
  stellarSecretKey: string;
  stellarNetwork: "testnet" | "mainnet";
  contractId: string;
}

export interface BallotLimits {
  maxTokens: number;
  maxVotes: number;
}

function getRpcUrl(network: string): string {
  return network === "mainnet" ? SOROBAN_RPC_MAINNET : SOROBAN_RPC_TESTNET;
}

function getNetworkPassphrase(network: string): string {
  return network === "mainnet"
    ? StellarSdk.Networks.PUBLIC
    : StellarSdk.Networks.TESTNET;
}

function getRpcServer(network: string): StellarSdk.SorobanRpc.Server {
  return new StellarSdk.SorobanRpc.Server(getRpcUrl(network), {
    allowHttp: false,
  });
}

export interface SorobanInvokeResult {
  txHash: string;
  success: boolean;
  returnValue?: unknown;
}

/**
 * Invoke a method on the deployed AnonVote Soroban contract.
 *
 * @param config  - Stellar credentials and contract ID
 * @param method  - Contract function name to call
 * @param args    - Arguments as native JS values (converted via nativeToScVal)
 * @returns txHash and return value, or empty string if not configured / fails
 */
export async function invokeContract(
  config: SorobanConfig,
  method: string,
  args: { value: unknown; type: string }[],
): Promise<SorobanInvokeResult> {
  if (!config.stellarSecretKey) {
    console.warn("[Soroban] No secret key configured, skipping contract call");
    return { txHash: "", success: false };
  }

  if (!config.contractId) {
    console.warn("[Soroban] No contract ID provided, skipping contract call");
    return { txHash: "", success: false };
  }

  try {
    const keypair = StellarSdk.Keypair.fromSecret(config.stellarSecretKey);
    const server = getRpcServer(config.stellarNetwork);
    const account = await server.getAccount(keypair.publicKey());

    const scArgs = args.map(({ value, type }) =>
      StellarSdk.nativeToScVal(value, { type: type as any }),
    );

    const contract = new StellarSdk.Contract(config.contractId);
    const operation = contract.call(method, ...scArgs);

    const tx = new StellarSdk.TransactionBuilder(account, {
      fee: StellarSdk.BASE_FEE,
      networkPassphrase: getNetworkPassphrase(config.stellarNetwork),
    })
      .addOperation(operation)
      .setTimeout(30)
      .build();

    const simulation = await server.simulateTransaction(tx);

    if (StellarSdk.SorobanRpc.Api.isSimulationError(simulation)) {
      console.error("[Soroban] Simulation failed:", simulation.error);
      return { txHash: "", success: false };
    }

    const preparedTx = StellarSdk.SorobanRpc.assembleTransaction(
      tx,
      simulation,
    ).build();

    preparedTx.sign(keypair);
    const sendResult = await server.sendTransaction(preparedTx);

    if (sendResult.status === "ERROR") {
      console.error("[Soroban] Send failed:", sendResult.errorResult);
      return { txHash: "", success: false };
    }

    const txHash = sendResult.hash;
    let getResult = await server.getTransaction(txHash);
    let attempts = 0;

    while (
      getResult.status ===
        StellarSdk.SorobanRpc.Api.GetTransactionStatus.NOT_FOUND &&
      attempts < 10
    ) {
      await new Promise((r) => setTimeout(r, 1500));
      getResult = await server.getTransaction(txHash);
      attempts++;
    }

    if (
      getResult.status ===
      StellarSdk.SorobanRpc.Api.GetTransactionStatus.SUCCESS
    ) {
      const returnValue = getResult.returnValue
        ? StellarSdk.scValToNative(getResult.returnValue)
        : undefined;
      console.log(`[Soroban] ${method} succeeded — tx: ${txHash}`);
      return { txHash, success: true, returnValue };
    }

    console.error("[Soroban] Transaction failed:", getResult);
    return { txHash, success: false };
  } catch (err) {
    console.error("[Soroban] invokeContract error:", err);
    return { txHash: "", success: false };
  }
}

/**
 * Read contract data without submitting a transaction (view call / simulation only).
 */
export async function readContract(
  config: SorobanConfig,
  method: string,
  args: { value: unknown; type: string }[],
): Promise<unknown | null> {
  if (!config.contractId) {
    console.warn("[Soroban] No contract ID provided, skipping read");
    return null;
  }

  try {
    const keypair = config.stellarSecretKey
      ? StellarSdk.Keypair.fromSecret(config.stellarSecretKey)
      : StellarSdk.Keypair.random();

    const server = getRpcServer(config.stellarNetwork);
    const account = await server.getAccount(keypair.publicKey());

    const scArgs = args.map(({ value, type }) =>
      StellarSdk.nativeToScVal(value, { type: type as any }),
    );

    const contract = new StellarSdk.Contract(config.contractId);
    const operation = contract.call(method, ...scArgs);

    const tx = new StellarSdk.TransactionBuilder(account, {
      fee: StellarSdk.BASE_FEE,
      networkPassphrase: getNetworkPassphrase(config.stellarNetwork),
    })
      .addOperation(operation)
      .setTimeout(30)
      .build();

    const simulation = await server.simulateTransaction(tx);

    if (StellarSdk.SorobanRpc.Api.isSimulationError(simulation)) {
      console.error("[Soroban] Read simulation failed:", simulation.error);
      return null;
    }

    if (
      StellarSdk.SorobanRpc.Api.isSimulationSuccess(simulation) &&
      simulation.result?.retval
    ) {
      return StellarSdk.scValToNative(simulation.result.retval);
    }

    return null;
  } catch (err) {
    console.error("[Soroban] readContract error:", err);
    return null;
  }
}

// ── AnonVote contract helpers ─────────────────────────────────────────────────
// These wrap invokeContract/readContract with the specific AnonVote contract
// methods. Import these in core's services once SOROBAN_CONTRACT_ID is set.

/**
 * Record a ballot creation on-chain.
 * Call from ballotEngine.createBallot() after the ballot is saved to DB.
 * ballotIdHash = hashIdentifier(ballotId) from @anonvote/crypto
 */
export async function sorobanRecordBallot(
  config: SorobanConfig,
  ballotIdHash: string,
  limits: BallotLimits,
): Promise<string> {
  if (!config.contractId) return "";
  const result = await invokeContract(config, "record_ballot", [
    { value: ballotIdHash, type: "string" },
    {
      value: { max_tokens: limits.maxTokens, max_votes: limits.maxVotes },
      type: "map",
    },
  ]);
  return result.txHash;
}

/**
 * Record a token issuance on-chain.
 * Call from identityManager.issueToken() after the token is issued.
 */
export async function sorobanRecordToken(
  config: SorobanConfig,
  ballotIdHash: string,
): Promise<string> {
  if (!config.contractId) return "";
  const result = await invokeContract(config, "record_token", [
    { value: ballotIdHash, type: "string" },
  ]);
  return result.txHash;
}

/**
 * Record a vote cast on-chain.
 * Call from privacyEngine.submitVote() after the vote is saved to DB.
 */
export async function sorobanRecordVote(
  config: SorobanConfig,
  ballotIdHash: string,
): Promise<string> {
  if (!config.contractId) return "";
  const result = await invokeContract(config, "record_vote", [
    { value: ballotIdHash, type: "string" },
  ]);
  return result.txHash;
}

/**
 * Record a result publication on-chain.
 * Call from resultEngine.tallyBallot() after the result is saved to DB.
 * resultHash: SHA-256 of the tally JSON string.
 */
export async function sorobanRecordResult(
  config: SorobanConfig,
  ballotIdHash: string,
  resultHash: string,
): Promise<string> {
  if (!config.contractId) return "";
  const result = await invokeContract(config, "record_result", [
    { value: ballotIdHash, type: "string" },
    { value: resultHash, type: "string" },
  ]);
  return result.txHash;
}

/**
 * Read on-chain audit counts for a ballot (view call — no transaction).
 */
export async function sorobanGetAuditCounts(
  config: SorobanConfig,
  ballotIdHash: string,
): Promise<{
  tokensIssued: number;
  votesCast: number;
  isConsistent: boolean;
} | null> {
  if (!config.contractId) return null;
  const [tokens, votes, consistent] = await Promise.all([
    readContract(config, "get_tokens_issued", [
      { value: ballotIdHash, type: "string" },
    ]),
    readContract(config, "get_votes_cast", [
      { value: ballotIdHash, type: "string" },
    ]),
    readContract(config, "is_consistent", [
      { value: ballotIdHash, type: "string" },
    ]),
  ]);
  return {
    tokensIssued: (tokens as number) ?? 0,
    votesCast: (votes as number) ?? 0,
    isConsistent: (consistent as boolean) ?? false,
  };
}
