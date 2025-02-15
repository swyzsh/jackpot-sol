#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
  program::invoke_signed, system_instruction, sysvar::clock::Clock,
};
use solana_program::hash::hash;
use solana_program::rent::Rent;
use std::str::FromStr;

const BUYBACK_ADDY: &str = "4o91wiYAsmtnpHbyaobF9q1vmswhY8kKKoSej8qtkRqv";
const FEE_ADDY: &str = "A3VipY34fosfdigEx4dDHjdwaaj1AnwrNgjbbGZuL7Y9";

declare_id!("AnrihJB9TT6WH12NPbch53KDrxQfzX5PrG1qDdnTcRiQ");

#[program]
pub mod jackpot {
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
    pot.winner = None;
    return Ok(());
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
    return Ok(());
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
    return Ok(());
  }

  // Ends the current round; Can only be called if the round was Active for
  // at least ACTIVE_DURATION seconds. This triggers a randomness request,
  // selects and stores the winner address and sets the game state to Cooldown.
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

    // Select a winner
    if pot.total_amount > 0 && !pot.deposits.is_empty() {
      let winner_index = (random_hash.to_bytes()[0] as usize) % pot.deposits.len();
      let winner_pubkey = pot.deposits[winner_index].depositor;
      msg!("Selected winner: {}", winner_pubkey);
      pot.winner = Some(winner_pubkey);
    } else {
      pot.winner = None;
    }

    pot.game_state = GameState::Cooldown;
    msg!("Round ended; Game state set to Cooldown");
    Ok(())
  }

  // Resets the pot if the pot is in Cooldown and there is no winner.
  pub fn reset_pot_if_no_winner(ctx: Context<ResetPotIfNoWinner>) -> Result<()> {
    let pot = &mut ctx.accounts.pot;

    require!(
      pot.game_state == GameState::Cooldown,
      ErrorCode::InvalidState
    );
    require!(pot.winner.is_none(), ErrorCode::InvalidWinnerAccount);

    // Reset the pot for next round.
    msg!("No winners this round; resetting state to inactive.");
    pot.game_state = GameState::Inactive;
    pot.total_amount = 0;
    pot.deposits.clear();
    pot.randomness = None;
    pot.winner = None;
    pot.last_reset = Clock::get()?.unix_timestamp;
    return Ok(());
  }

  // Distributes rewards and reset the game state
  pub fn distribute_rewards(ctx: Context<DistributeRewards>) -> Result<()> {
    let pot = &mut ctx.accounts.pot;

    // Ensure game is in Cooldown and Randomness is available.
    require!(
      pot.game_state == GameState::Cooldown,
      ErrorCode::InvalidState
    );
    require!(pot.randomness.is_some(), ErrorCode::RandomnessNotAvailable);

    // If no deposits or no winner, skip distributiona and reset pot.
    if pot.total_amount == 0 || pot.deposits.is_empty() || pot.winner.is_none() {
      msg!("No deposits found; Skipping distribution. Resetting pot state...");
      pot.game_state = GameState::Inactive;
      pot.total_amount = 0;
      pot.deposits.clear();
      pot.randomness = None;
      pot.winner = None;
      pot.last_reset = Clock::get()?.unix_timestamp;
      return Ok(());
    }

    let stored_winner = pot.winner.unwrap();
    require!(
      ctx.accounts.winner.key() == stored_winner,
      ErrorCode::InvalidWinnerAccount
    );

    let buyback_address =
      Pubkey::from_str(BUYBACK_ADDY).expect("Hardcoded buyback address is invalid");
    require!(
      ctx.accounts.buyback.key() == buyback_address,
      ErrorCode::InvalidBuybackAccount
    );

    let fee_address = Pubkey::from_str(FEE_ADDY).expect("Hardcoded fee address is invalid");
    require!(
      ctx.accounts.fee.key() == fee_address,
      ErrorCode::InvalidFeeAccount
    );

    // Calculate POT PDA's rent exempt minimum and safe gaurd from total_amount.
    let rent = Rent::get()?;
    let pot_account_info = pot.to_account_info();
    let pot_account_size = pot_account_info.data_len();
    let rent_exempt_minimum = rent.minimum_balance(pot_account_size);
    let safe_guard: u64 = 100_000_000; // 0.1 sol

    // Calculate distributable amount after accounting for rent exempt and safe guard.
    let total_amount = pot.total_amount;
    let distributable_amount = total_amount
      .checked_sub(rent_exempt_minimum)
      .ok_or(ErrorCode::InsufficientFundsForRent)?;
    let distributable_amount = distributable_amount
      .checked_sub(safe_guard)
      .ok_or(ErrorCode::InsufficientFundsForRent)?;

    let winner_amount = distributable_amount * 970 / 1000;
    let buyback_amount = distributable_amount * 25 / 1000;
    let fee_amount = distributable_amount * 5 / 1000;

    // PDA cannot do system CPI, so directly adjust lamports.
    {
      let winner_info = &mut ctx.accounts.winner.to_account_info();
      let buyback_info = &mut ctx.accounts.buyback.to_account_info();
      let fee_info = &mut ctx.accounts.fee.to_account_info();
      // Transfer to winner.
      **pot_account_info.try_borrow_mut_lamports()? -= winner_amount;
      **winner_info.try_borrow_mut_lamports()? += winner_amount;
      // Transfer to buyback.
      **pot_account_info.try_borrow_mut_lamports()? -= buyback_amount;
      **buyback_info.try_borrow_mut_lamports()? += buyback_amount;
      // Transfer to fee.
      **pot_account_info.try_borrow_mut_lamports()? -= fee_amount;
      **fee_info.try_borrow_mut_lamports()? += fee_amount;
    }

    // Reset the pot for next round.
    pot.game_state = GameState::Inactive;
    pot.total_amount = 0;
    pot.deposits.clear();
    pot.randomness = None;
    pot.winner = None;
    pot.last_reset = Clock::get()?.unix_timestamp;
    msg!("Rewards distributed; Game state reset to Inactive");
    return Ok(());
  }

  // Ensure the game is NOT in Active state to avoid interfering with an active round
  pub fn admin_withdraw(ctx: Context<AdminWithdraw>) -> Result<()> {
    let pot = &mut ctx.accounts.pot;

    require!(
      pot.game_state != GameState::Active,
      ErrorCode::CannotWithdrawDuringActive
    );

    let rent = Rent::get()?;
    let min_rent = rent.minimum_balance(pot.to_account_info().data_len());

    let pot_lamports = pot.to_account_info().lamports();
    msg!("Admin withdraw: pot has {} lamports", pot_lamports);

    if pot_lamports > min_rent {
      let withdraw_amount = pot_lamports - min_rent;
      **pot.to_account_info().try_borrow_mut_lamports()? = min_rent;
      **ctx.accounts.fee.try_borrow_mut_lamports()? += withdraw_amount;
      msg!(
        "Transferred {} lamports from Pot PDA to Fee address.",
        withdraw_amount
      );
    } else {
      msg!("Pot has insufficient lamports above rent-exempt minimum; skipping transfer.");
    }

    // Reset the deposits for next round.
    pot.total_amount = 0;
    pot.deposits.clear();
    // pot.randomness = None;
    // pot.winner = None;
    return Ok(());
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
pub struct ResetPotIfNoWinner<'info> {
  #[account(mut, seeds = [b"pot"], bump)]
  pub pot: Account<'info, Pot>,
  // // If want only admin to be able to do this, keep it:
  // #[account(mut)]
  // pub admin: Signer<'info>,
  pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DistributeRewards<'info> {
  #[account(mut, seeds = [b"pot"], bump)]
  pub pot: Account<'info, Pot>,

  /// CHECK: Verified in code
  #[account(mut)]
  pub winner: UncheckedAccount<'info>,

  // Hardcoded buyback
  /// CHECK: Verified in code
  #[account(mut)]
  pub buyback: UncheckedAccount<'info>,

  // Hardcoded fee
  /// CHECK: Verified in code
  #[account(mut)]
  pub fee: UncheckedAccount<'info>,

  pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AdminWithdraw<'info> {
  #[account(mut, seeds = [b"pot"], bump)]
  pub pot: Account<'info, Pot>,

  // Ensure ADMIN only.
  #[account(mut)]
  pub admin: Signer<'info>,

  #[account(mut, address = Pubkey::from_str(FEE_ADDY).unwrap())]
  /// CHECK: We trust this is the correct fee address
  pub fee: UncheckedAccount<'info>,

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
  pub winner: Option<Pubkey>,
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
  #[msg("Game is not active.")]
  GameInactive,
  #[msg("Minimum deposit not met.")]
  MinDeposit,
  #[msg("Invalid state for operation.")]
  InvalidState,
  #[msg("Cooldown still active.")]
  CooldownActive,
  #[msg("No deposits found.")]
  NoDeposits,
  #[msg("Randomness not available.")]
  RandomnessNotAvailable,
  #[msg("Winner account does not match pot.winner")]
  InvalidWinnerAccount,
  #[msg("Buyback account does not match the hardcoded address")]
  InvalidBuybackAccount,
  #[msg("Fee account does not match the hardcoded address")]
  InvalidFeeAccount,
  #[msg("Cannot withdraw while game is active.")]
  CannotWithdrawDuringActive,
  #[msg("Insufficient funds to leave rent-exempt")]
  InsufficientFundsForRent,
}
