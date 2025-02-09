#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
  program::invoke_signed, system_instruction, sysvar::clock::Clock,
};
use dotenvy::dotenv;
use solana_program::hash::hash;
use std::env;
use std::str::FromStr;

fn get_pubkey_from_env(var_name: &str) -> Pubkey {
  env::var(var_name)
    .expect(&format!("{} is not set", var_name))
    .parse::<Pubkey>()
    .expect(&format!("Invalid pubkey format for {}", var_name))
}

declare_id!("HtbKartrbcGdW3wfhV2WsZVE4ybHhkKqWUr7V6PwEgfZ");

#[program]
pub mod jackpot_program {
  use super::*;

  // Initialize the Pot account.
  // Game is Inactive at the start.
  pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
    let pot = &mut ctx.accounts.pot;
    let (pda, bump) = Pubkey::find_program_address(&[b"pot"], &ID);
    msg!("Initialized Pot PDA: {}", pda);
    msg!("Bump: {}", bump);
    pot.admin = ctx.accounts.admin.key();
    pot.bump = bump;
    pot.game_state = GameState::Inactive;
    pot.last_reset = Clock::get()?.unix_timestamp;
    pot.total_amount = 0;
    pot.deposits = vec![];
    pot.end_game_caller = None;
    Ok(())
  }

  // Starts a new round. Can only be called when the game is Inactive
  // & Cooldown period has passed.
  pub fn start_round(ctx: Context<StartRound>) -> Result<()> {
    let pot = &mut ctx.accounts.pot;
    let clock = Clock::get()?;

    require!(
      pot.game_state == GameState::Inactive,
      ErrorCode::InvalidState
    ); // Ensure game is Inactive.
    require!(
      clock.unix_timestamp - pot.last_reset >= Pot::COOLDOWN_DURATION,
      ErrorCode::CooldownActive
    ); // Ensure Cooldown period has passed.

    pot.game_state = GameState::Active;
    pot.last_reset = clock.unix_timestamp;
    msg!("Round started at: {}", pot.last_reset);
    Ok(())
  }

  // Accepts a deposit from a user.
  // Only allowed when the game is Active.
  pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    let pot = &mut ctx.accounts.pot;
    let clock = Clock::get()?;

    require!(pot.game_state == GameState::Active, ErrorCode::GameInactive); // Ensure game is Active.
    require!(amount >= 50_000_000, ErrorCode::MinDeposit); // 0.05 SOL minimum

    // Transfer SOL from the user to the Pot PDA.
    let transfer_ix = system_instruction::transfer(&ctx.accounts.user.key(), &pot.key(), amount);
    invoke_signed(
      &transfer_ix,
      &[
        ctx.accounts.user.to_account_info(),
        pot.to_account_info(),
        ctx.accounts.system_program.to_account_info(),
      ],
      &[],
    )?;

    // Record the deposit.
    pot.deposits.push(DepositRecord {
      depositor: ctx.accounts.user.key(),
      amount,
      timestamp: clock.unix_timestamp,
    });
    pot.total_amount += amount;
    msg!("Deposits of {} lamports accepted", amount);
    Ok(())
  }

  // Ends the current round.
  // Can only be called if the round was Active for at least ACTIVE_DURATION seconds.
  // This triggers a randomness request and sets the game state to Cooldown.
  pub fn end_round(ctx: Context<EndRound>) -> Result<()> {
    let pot = &mut ctx.accounts.pot;
    let clock = Clock::get()?;

    require!(pot.game_state == GameState::Active, ErrorCode::InvalidState); // Ensure game is Active
    require!(
      clock.unix_timestamp - pot.last_reset >= Pot::ACTIVE_DURATION,
      ErrorCode::CooldownActive
    ); // Active past than ACTIVE_DURATION

    // Generate Pseudo-Randomness by hashing together some on-chain data...
    let seed_data = [
      pot.key().to_bytes().as_ref(),       // Pot PDA
      &clock.unix_timestamp.to_le_bytes(), // Current Time
      &pot.total_amount.to_le_bytes(),     // Pot size
      &[pot.bump],                         // Pot bump
    ]
    .concat();
    let random_hash = hash(&seed_data);
    pot.randomness = Some(random_hash.to_bytes());

    // WARNING: Remove the hash msg! in production
    msg!("Pseudo-random hash: {:?}", random_hash);
    pot.end_game_caller = Some(ctx.accounts.caller.key());
    pot.game_state = GameState::Cooldown;
    msg!("Round ended; Game state set to Cooldown");
    Ok(())
  }

  // Distributes rewards based on the randomness, resets the game state,
  // clears deposits, and updates the last_reset timestamp.
  pub fn distribute_rewards(ctx: Context<DistributeRewards>) -> Result<()> {
    let pot = &mut ctx.accounts.pot;

    require!(
      pot.game_state == GameState::Cooldown,
      ErrorCode::InvalidState
    ); // Ensure game is in Cooldown
    require!(pot.randomness.is_some(), ErrorCode::RandomnessNotAvailable); // Ensure randomness
    let total_amount = pot.total_amount;
    require!(total_amount > 0, ErrorCode::NoDeposits); // Ensure some deposits

    // Select a winner using the randomness result
    let randomness = pot.randomness.unwrap();
    // Take the first byte mod the length of deposits
    let winner_index = (randomness[0] as usize) % pot.deposits.len();
    let winner = pot.deposits[winner_index].depositor;

    let winner_amount = total_amount * 969 / 1000;
    let buyback_amount = total_amount * 25 / 1000;
    let fee_amount = total_amount * 5 / 1000;
    let caller_bonus = total_amount * 1 / 1000;

    dotenv().ok(); // Load .env variables
    let buyback_address = get_pubkey_from_env("BUYBACK_ADDRESS");
    let fee_address = get_pubkey_from_env("FEE_ADDRESS");
    let caller_address = pot.end_game_caller.unwrap();

    let transfers = vec![
      (winner, winner_amount),
      (buyback_address, buyback_amount),
      (fee_address, fee_amount),
      (caller_address, caller_bonus),
    ];

    for (recipient, amount) in transfers {
      let transfer_ix = system_instruction::transfer(&pot.key(), &recipient, amount);
      invoke_signed(
        &transfer_ix,
        &[
          pot.to_account_info(),
          ctx.accounts.system_program.to_account_info(),
        ],
        &[],
      )?;
    }

    // Reset the pot for next round.
    pot.game_state = GameState::Inactive;
    pot.total_amount = 0;
    pot.deposits.clear();
    pot.randomness = None;
    pot.end_game_caller = None;
    pot.last_reset = Clock::get()?.unix_timestamp;
    msg!("Rewards distributed; Game state reset to Inactive");
    Ok(())
  }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
  #[account(init, payer = admin, space = 10240, seeds = [b"pot"], bump)]
  pub pot: Account<'info, Pot>,
  // Restrict this to admin only.
  #[account(mut)]
  pub admin: Signer<'info>,
  pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct StartRound<'info> {
  #[account(mut, seeds = [b"pot"], bump)]
  pub pot: Account<'info, Pot>,
  // Restrict this to admin only.
  #[account(mut)]
  pub admin: Signer<'info>,
  pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
  #[account(mut, seeds = [b"pot"], bump)]
  pub pot: Account<'info, Pot>,
  #[account(mut)]
  pub user: Signer<'info>,
  pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct EndRound<'info> {
  #[account(mut, seeds = [b"pot"], bump)]
  pub pot: Account<'info, Pot>,
  #[account(mut)]
  pub caller: Signer<'info>,
  pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DistributeRewards<'info> {
  #[account(mut, seeds = [b"pot"], bump)]
  pub pot: Account<'info, Pot>,
  pub system_program: Program<'info, System>,
}

#[account]
pub struct Pot {
  pub admin: Pubkey,
  pub bump: u8,
  pub total_amount: u64,
  pub deposits: Vec<DepositRecord>,
  pub game_state: GameState,
  pub last_reset: i64,
  pub randomness: Option<[u8; 32]>,
  pub end_game_caller: Option<Pubkey>,
}

impl Pot {
  pub const ACTIVE_DURATION: i64 = 120; // 120 seconds
  pub const COOLDOWN_DURATION: i64 = 360; // 360 seconds
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct DepositRecord {
  pub depositor: Pubkey,
  pub amount: u64,
  pub timestamp: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Debug)]
pub enum GameState {
  Active,
  Cooldown,
  Inactive,
}

#[error_code]
pub enum ErrorCode {
  GameInactive,
  MinDeposit,
  InvalidState,
  CooldownActive,
  NoDeposits,
  RandomnessNotAvailable,
}
