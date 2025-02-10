import * as anchor from "@coral-xyz/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import * as idl from "../target/idl/jackpot.json";
import { Jackpot } from "../target/types/jackpot";

const ACTIVE_DURATION = 120;
const COOLDOWN_DURATION = 360;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function getGameState(state: any): string {
  return typeof state === "string" ? state : Object.keys(state)[0];
}

async function runScheduler() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = new anchor.Program<Jackpot>(idl as Jackpot, provider);
  const [POT_PDA, bump] = PublicKey.findProgramAddressSync(
    [Buffer.from("pot")],
    program.programId
  );
  console.log("Pot PDA:", POT_PDA.toBase58(), "|", "Bump:", bump);

  const nowSec = () => Math.floor(Date.now() / 1000);

  while (true) {
    try {
      const pot = await program.account.pot.fetch(POT_PDA);
      const lastReset: number =
        typeof pot.lastReset === "number"
          ? pot.lastReset
          : new anchor.BN(pot.lastReset).toNumber();
      const now: number = nowSec();
      console.log(
        "Current game state:",
        getGameState(pot.gameState),
        "|",
        "Last reset:",
        lastReset,
        "|",
        "Now:",
        now
      );

      switch (getGameState(pot.gameState)) {
        case "inactive":
          if (now - lastReset >= COOLDOWN_DURATION) {
            console.log("Cooldown complete. Starting new round...");
            const tx = await program.methods
              .startRound()
              .accounts({
                admin: provider.wallet.publicKey,
              })
              .rpc();
            console.log("New round started. Tx Signature:", tx);
          } else {
            console.log("Inactive state; cooldown not yet over.");
          }
          break;
        case "active":
          if (now - lastReset >= ACTIVE_DURATION) {
            console.log("Active duration complete. Ending round...");
            const caller = provider.wallet.publicKey;
            const tx = await program.methods
              .endRound()
              .accounts({
                caller: caller,
              })
              .rpc();
            console.log(
              "Round ended; game state set to Cooldown. Tx Signature:",
              tx
            );
          } else {
            console.log("Round is active and within active duration.");
          }
          break;
        case "cooldown":
          if (pot.randomness) {
            console.log(
              "Randomness available. Attempting to distribute rewards..."
            );
            const tx = await program.methods
              .distributeRewards()
              .accounts({
                systemProgram: SystemProgram.programId,
              })
              .rpc();
            console.log(
              "DistributeRewards completed; Game state reset to Inactive. Tx Signature:",
              tx
            );
          } else {
            console.log(
              "Game in cooldown; waiting for randomness fulfillment..."
            );
          }
          break;

        default:
          console.log("Unknown game state:", pot.gameState);
      }
    } catch (error) {
      console.error("Error during scheduler loop:", error);
    }
    await sleep(5000); // Wait for 5 seconds before checking again...
  }
}

runScheduler().catch(console.error);
