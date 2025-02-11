#![allow(unexpected_cfgs)]
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
  program::invoke_signed, system_instruction, sysvar::clock::Clock,
};
use solana_program::hash::hash;
use std::str::FromStr;

const BUYBACK_ADDY: &str = "4o91wiYAsmtnpHbyaobF9q1vmswhY8kKKoSej8qtkRqv";
const FEE_ADDY: &str = "A3VipY34fosfdigEx4dDHjdwaaj1AnwrNgjbbGZuL7Y9";

declare_id!("9ux74QJi5pxgXNnBo3n16YCVj8GM6MDVyYpuLSfV2uSJ");

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
    pot.end_game_caller = None;
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
    pot.end_game_caller = Some(ctx.accounts.caller.key());

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
    require!(
      pot.total_amount == 0 || pot.deposits.is_empty(),
      ErrorCode::PotNotEmpty
    );

    // Reset the pot for next round.
    msg!("No winners this round; resetting state to inactive.");
    pot.game_state = GameState::Inactive;
    pot.total_amount = 0;
    pot.deposits.clear();
    pot.randomness = None;
    pot.end_game_caller = None;
    pot.winner = None;
    pot.last_reset = Clock::get()?.unix_timestamp;
    return Ok(());
  }

  // Distributes rewards and reset the game state
  pub fn distribute_rewards(ctx: Context<DistributeRewards>) -> Result<()> {
    let pot = &mut ctx.accounts.pot;

    require!(
      pot.game_state == GameState::Cooldown,
      ErrorCode::InvalidState
    ); // Ensure game is in Cooldown
    require!(pot.randomness.is_some(), ErrorCode::RandomnessNotAvailable); // Ensure randomness

    // If no deposits or no winner, skip distributiona.
    if pot.total_amount == 0 || pot.deposits.is_empty() || pot.winner.is_none() {
      msg!("No deposits found; Skipping distribution. Resetting pot state...");
      pot.game_state = GameState::Inactive;
      pot.total_amount = 0;
      pot.deposits.clear();
      pot.randomness = None;
      pot.end_game_caller = None;
      pot.winner = None;
      pot.last_reset = Clock::get()?.unix_timestamp;
      return Ok(());
    }

    let total_amount = pot.total_amount;
    let winner_amount = total_amount * 969 / 1000;
    let buyback_amount = total_amount * 25 / 1000;
    let fee_amount = total_amount * 5 / 1000;
    let caller_bonus = total_amount * 1 / 1000;

    let buyback_address =
      Pubkey::from_str(BUYBACK_ADDY).expect("Hardcoded buyback address is invalid");
    let fee_address = Pubkey::from_str(FEE_ADDY).expect("Hardcoded fee address is invalid");

    let stored_winner = match pot.winner {
      Some(pw) => pw,
      None => {
        msg!("No winner found; Skipping distribution.");
        // Reset the pot for next round.
        pot.game_state = GameState::Inactive;
        pot.total_amount = 0;
        pot.deposits.clear();
        pot.randomness = None;
        pot.end_game_caller = None;
        pot.winner = None;
        pot.last_reset = Clock::get()?.unix_timestamp;
        return Ok(());
      }
    };

    let stored_caller = match pot.end_game_caller {
      Some(sc) => sc,
      None => {
        msg!("No end_game_caller found; Skipping distribution.");
        // Reset the pot for next round.
        pot.game_state = GameState::Inactive;
        pot.total_amount = 0;
        pot.deposits.clear();
        pot.randomness = None;
        pot.end_game_caller = None;
        pot.winner = None;
        pot.last_reset = Clock::get()?.unix_timestamp;
        return Ok(());
      }
    };

    require!(
      ctx.accounts.winner.key() == stored_winner,
      ErrorCode::InvalidWinnerAccount
    );
    require!(
      ctx.accounts.caller.key() == stored_caller,
      ErrorCode::InvalidCallerAccount
    );
    require!(
      ctx.accounts.buyback.key() == buyback_address,
      ErrorCode::InvalidBuybackAccount
    );
    require!(
      ctx.accounts.fee.key() == fee_address,
      ErrorCode::InvalidFeeAccount
    );

    let transfer_list = vec![
      (ctx.accounts.winner.key(), winner_amount),
      (ctx.accounts.buyback.key(), buyback_amount),
      (ctx.accounts.fee.key(), fee_amount),
      (ctx.accounts.caller.key(), caller_bonus),
    ];

    for (recipient_pubkey, lamports) in transfer_list {
      let ix = system_instruction::transfer(&pot.key(), &recipient_pubkey, lamports);
      invoke_signed(
        &ix,
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

    let pot_lamports = pot.to_account_info().lamports();
    msg!("Admin withdraw: pot has {} lamports", pot_lamports);

    let fee_address = Pubkey::from_str(FEE_ADDY).expect("Hardcoded fee address invalid");

    if pot_lamports > 0 {
      let ix = system_instruction::transfer(&pot.key(), &fee_address, pot_lamports);
      invoke_signed(
        &ix,
        &[
          pot.to_account_info(),
          ctx.accounts.system_program.to_account_info(),
        ],
        &[],
      )?;
      msg!(
        "Transferred {} lamports from Pot PDA to Fee address.",
        pot_lamports
      );
    } else {
      msg!("Pot has 0 lamports; skipping transfer.");
    }

    // Reset the deposits for next round.
    pot.total_amount = 0;
    pot.deposits.clear();
    // pot.randomness = None;
    // pot.end_game_caller = None;
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

  // The winner from pot.winner
  #[account(mut)]
  /// CHECK: We verify in code that `winner.key() == pot.winner`
  pub winner: UncheckedAccount<'info>,

  // The end_game_caller from pot.end_game_caller
  #[account(mut)]
  /// CHECK: We verify in code that `caller.key() == pot.end_game_caller`
  pub caller: UncheckedAccount<'info>,

  // Hardcoded buyback
  #[account(mut, address = Pubkey::from_str(BUYBACK_ADDY).unwrap())]
  /// CHECK: We check this matches the const above
  pub buyback: UncheckedAccount<'info>,

  // Hardcoded fee
  #[account(mut, address = Pubkey::from_str(FEE_ADDY).unwrap())]
  /// CHECK: We check this matches the const above
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
  pub end_game_caller: Option<Pubkey>,
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
  #[msg("Pot is not empty; cannot reset.")]
  PotNotEmpty,
  #[msg("Caller account does not match pot.end_game_caller")]
  InvalidCallerAccount,
  #[msg("Buyback account does not match the hardcoded address")]
  InvalidBuybackAccount,
  #[msg("Fee account does not match the hardcoded address")]
  InvalidFeeAccount,
  #[msg("Cannot withdraw while game is active.")]
  CannotWithdrawDuringActive,
}
