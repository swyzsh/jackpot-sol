import * as anchor from "@coral-xyz/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";

async function main() {
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

  const tx = await program.methods
    .initialize()
    .accounts({
      pot: POT_PDA,
      admin: provider.wallet.publicKey,
      systemProgram: SystemProgram.programId,
    })
    .rpc();

  console.log("Transaction signature:", tx);
}

console.log("Running initialization script...");

main()
  .then(() => {
    console.log("Initialization complete");
    process.exit(0);
  })
  .catch((err) => {
    console.error("Initialization failed:", err);
    process.exit(1);
  });

// To set Environment Variables:
// export ANCHOR_WALLET=~/.config/solana/id.json
// export ANCHOR_PROVIDER_URL=https://api.devnet.solana.com
