use anchor_lang::prelude::*;

declare_id!("EF1MGYz7Wo3zjnVdNgwxf3DtPZSaBGVLuofzu6PQp9da");

#[program]
pub mod jackpot {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
