import * as anchor from "@coral-xyz/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import * as idl from "../target/idl/jackpot.json";
import { Jackpot } from "../target/types/jackpot";

const FEE_ADDY: string = "A3VipY34fosfdigEx4dDHjdwaaj1AnwrNgjbbGZuL7Y9";

async function withdrawPot() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  // const program = new anchor.Program<Jackpot>(idl as Jackpot, provider);
  const program = anchor.workspace.Jackpot;
  if (!program) {
    throw new Error(
      "Program not found in workspace. Did you build your program?"
    );
  }
  console.log("Program ID:", program.programId.toBase58());

  const [POT_PDA, bump] = PublicKey.findProgramAddressSync(
    [Buffer.from("pot")],
    program.programId
  );
  console.log("Pot PDA:", POT_PDA.toBase58(), "|", "Bump:", bump);

  const feePubkey = new PublicKey(FEE_ADDY);

  console.log("Withdrawing all lamports from POT PDA to the Fee Address...");
  try {
    // Emergency Withdraw - DOES NOT work during active rounds.
    const tx = await program.methods
      .adminWithdraw()
      .accounts({
        pot: POT_PDA,
        admin: provider.wallet.publicKey,
        fee: feePubkey,
        systemProgram: SystemProgram.programId,
      } as any)
      .rpc();
    console.log("Emergency withdraw successful. Tx:", tx);
  } catch (err) {
    console.error("Failed to withdraw pot:", err);
  }
}

withdrawPot()
  .then(() => {
    console.log("withdrawPot script completed.");
    process.exit(0);
  })
  .catch((err) => {
    console.error("withdrawPot script error:", err);
    process.exit(1);
  });
