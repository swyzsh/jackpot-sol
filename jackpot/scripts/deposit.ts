import * as anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { Jackpot } from "../target/types/jackpot";
import * as idl from "../target/idl/jackpot.json";

async function main() {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = new anchor.Program<Jackpot>(idl as Jackpot, provider);
  const [POT_PDA, bump] = PublicKey.findProgramAddressSync(
    [Buffer.from("pot")],
    program.programId
  );
  console.log("Pot PDA:", POT_PDA.toBase58(), "Bump:", bump);

  const depositLamports = 0.05 * anchor.web3.LAMPORTS_PER_SOL;
  console.log(
    `Depositing 0.05 SOL or ${depositLamports} lamports into the pot...`
  );
  const tx = await program.methods
    .deposit(new anchor.BN(depositLamports))
    .accounts({ user: provider.wallet.publicKey })
    .rpc();
  console.log("Deposit transaction signature:", tx);
}

main()
  .then(() => {
    console.log("Deposit script completed successfully.");
    process.exit(0);
  })
  .catch((err) => {
    console.error("Deposit script failed:", err);
    process.exit(1);
  });
