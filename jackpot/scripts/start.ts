import * as anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";

async function runStart() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.Jackpot;
  if (!program) {
    throw new Error(
      "Program not found in workspace. Make sure you've built your program."
    );
  }

  console.log("Program ID:", program.programId.toBase58());

  const [POT_PDA, bump] = PublicKey.findProgramAddressSync(
    [Buffer.from("pot")],
    program.programId
  );
  console.log("Pot PDA:", POT_PDA.toBase58());
  console.log("Bump:", bump);

  console.log("Starting game round...");

  const tx = await program.methods
    .startRound()
    .accounts({ admin: provider.wallet.publicKey })
    .rpc();
  console.log("Transaction signature:", tx);
}

console.log("Trying to Start the game round...");
runStart()
  .then(() => {
    console.log("Game round started successfully! ^^");
    process.exit(0);
  })
  .catch((err) => {
    console.error("Game round failed to start! :/ ", err);
    process.exit(1);
  });
