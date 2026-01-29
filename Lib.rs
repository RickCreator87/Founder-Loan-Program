use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS"); // Replace with your deployment

#[program]
pub mod founder_loan_program {
    use super::*;

    // Initialize protocol configuration
    pub fn initialize_protocol(
        ctx: Context<InitializeProtocol>,
        protocol_fee_bps: u16,        // Basis points (e.g., 50 = 0.5%)
        min_credit_score: u16,        // Starting score for new borrowers
    ) -> Result<()> {
        let config = &mut ctx.accounts.protocol_config;
        config.authority = ctx.accounts.authority.key();
        protocol_fee_bps;
        config.min_credit_score = min_credit_score;
        config.total_loans_created = 0;
        config.total_volume = 0;
        config.bump = *ctx.bumps.get("protocol_config").unwrap();
        
        msg!("Protocol initialized with fee: {} bps", protocol_fee_bps);
        Ok(())
    }

    // Create a new founder loan
    pub fn create_loan(
        ctx: Context<CreateLoan>,
        principal: u64,               // In USDC (6 decimals)
        repayment_percentage: u16,    // Basis points (2500 = 25%)
        term_months: Option<u16>,     // None = perpetual/open
        collateral_type: CollateralType,
    ) -> Result<()> {
        require!(repayment_percentage <= 5000, ErrorCode::InvalidPercentage); // Max 50%
        require!(principal >= 100_000_000, ErrorCode::PrincipalTooSmall); // Min $100 USDC
        
        let loan = &mut ctx.accounts.loan_account;
        let borrower_profile = &mut ctx.accounts.borrower_profile;
        let config = &ctx.accounts.protocol_config;
        
        // Initialize loan account
        loan.loan_id = config.total_loans_created + 1;
        loan.borrower = ctx.accounts.borrower.key();
        loan.lender = ctx.accounts.lender.key();
        loan.principal = principal;
        loan.repayment_percentage = repayment_percentage;
        loan.term_months = term_months;
        loan.collateral_type = collateral_type;
        loan.status = LoanStatus::Active;
        loan.amount_repaid = 0;
        loan.payments_made = 0;
        loan.credit_score = config.min_credit_score;
        loan.created_at = Clock::get()?.unix_timestamp;
        loan.last_payment_at = None;
        loan.bump = *ctx.bumps.get("loan_account").unwrap();
        
        // Link to borrower profile
        if borrower_profile.owner == Pubkey::default() {
            borrower_profile.owner = ctx.accounts.borrower.key();
            borrower_profile.total_loans = 0;
            borrower_profile.total_borrowed = 0;
            borrower_profile.total_repaid = 0;
            borrower_profile.current_credit_score = config.min_credit_score;
            borrower_profile.lifetime_credit_high = config.min_credit_score;
            borrower_profile.bump = *ctx.bumps.get("borrower_profile").unwrap();
        }
        
        borrower_profile.total_loans += 1;
        borrower_profile.total_borrowed += principal;
        borrower_profile.active_loans += 1;
        
        // Update protocol stats
        let config = &mut ctx.accounts.protocol_config;
        config.total_loans_created += 1;
        config.total_volume += principal;
        
        // Emit events for indexing
        emit!(LoanCreated {
            loan_id: loan.loan_id,
            borrower: loan.borrower,
            lender: loan.lender,
            principal,
            repayment_percentage,
            timestamp: loan.created_at,
        });
        
        msg!("Loan {} created: ${} at {}% repayment", 
            loan.loan_id, principal / 1_000_000, repayment_percentage / 100);
        
        Ok(())
    }

    // Process a repayment
    pub fn make_repayment(
        ctx: Context<MakeRepayment>,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, ErrorCode::InvalidAmount);
        
        let loan = &mut ctx.accounts.loan_account;
        let borrower_profile = &mut ctx.accounts.borrower_profile;
        
        require!(loan.status == LoanStatus::Active, ErrorCode::LoanNotActive);
        
        // Transfer USDC from borrower to lender
        let cpi_accounts = Transfer {
            from: ctx.accounts.borrower_token_account.to_account_info(),
            to: ctx.accounts.lender_token_account.to_account_info(),
            authority: ctx.accounts.borrower.to_account_info(),
        };
        
        token::transfer(
            CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts),
            amount,
        )?;
        
        // Update loan state
        let old_repaid = loan.amount_repaid;
        loan.amount_repaid += amount;
        loan.payments_made += 1;
        loan.last_payment_at = Some(Clock::get()?.unix_timestamp);
        
        // Calculate new credit score
        let old_score = loan.credit_score;
        let new_score = calculate_credit_score(loan, borrower_profile, amount);
        loan.credit_score = new_score;
        
        // Update borrower profile
        borrower_profile.total_repaid += amount;
        borrower_profile.current_credit_score = new_score;
        if new_score > borrower_profile.lifetime_credit_high {
            borrower_profile.lifetime_credit_high = new_score;
        }
        
        // Check if loan repaid
        if loan.amount_repaid >= loan.principal {
            loan.status = LoanStatus::Repaid;
            borrower_profile.active_loans -= 1;
            borrower_profile.completed_loans += 1;
            
            emit!(LoanRepaid {
                loan_id: loan.loan_id,
                borrower: loan.borrower,
                total_repaid: loan.amount_repaid,
                timestamp: Clock::get()?.unix_timestamp,
            });
        }
        
        // Collect protocol fee (optional)
        let fee_amount = (amount as u128)
            .checked_mul(ctx.accounts.protocol_config.protocol_fee_bps as u128)
            .unwrap()
            .checked_div(10000)
            .unwrap() as u64;
        
        if fee_amount > 0 {
            let fee_cpi = Transfer {
                from: ctx.accounts.borrower_token_account.to_account_info(),
                to: ctx.accounts.treasury_token_account.to_account_info(),
                authority: ctx.accounts.borrower.to_account_info(),
            };
            
            token::transfer(
                CpiContext::new(ctx.accounts.token_program.to_account_info(), fee_cpi),
                fee_amount,
            )?;
        }
        
        emit!(PaymentMade {
            loan_id: loan.loan_id,
            amount,
            fee: fee_amount,
            old_credit_score: old_score,
            new_credit_score: new_score,
            remaining_principal: loan.principal.saturating_sub(loan.amount_repaid),
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Payment made on loan {}: ${}, new score: {}", 
            loan.loan_id, amount / 1_000_000, new_score);
        
        Ok(())
    }

    // Automatic repayment from revenue (for Checkout-core integration)
    pub fn auto_repay_from_revenue(
        ctx: Context<AutoRepayFromRevenue>,
        revenue_amount: u64,
    ) -> Result<()> {
        let loan = &mut ctx.accounts.loan_account;
        
        require!(loan.status == LoanStatus::Active, ErrorCode::LoanNotActive);
        
        // Calculate repayment amount (e.g., 25% of revenue)
        let repayment_amount = (revenue_amount as u128)
            .checked_mul(loan.repayment_percentage as u128)
            .unwrap()
            .checked_div(10000)
            .unwrap() as u64;
        
        // Cap at remaining principal
        let remaining = loan.principal.saturating_sub(loan.amount_repaid);
        let actual_repayment = std::cmp::min(repayment_amount, remaining);
        
        if actual_repayment == 0 {
            return Ok(()); // Nothing to repay
        }
        
        // Transfer from company treasury
        let cpi_accounts = Transfer {
            from: ctx.accounts.company_treasury.to_account_info(),
            to: ctx.accounts.lender_token_account.to_account_info(),
            authority: ctx.accounts.company_authority.to_account_info(),
        };
        
        token::transfer(
            CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts),
            actual_repayment,
        )?;
        
        // Update state (same as manual repayment)
        loan.amount_repaid += actual_repayment;
        loan.payments_made += 1;
        loan.last_payment_at = Some(Clock::get()?.unix_timestamp);
        
        // Recalculate credit score
        let new_score = calculate_credit_score(loan, &mut ctx.accounts.borrower_profile, actual_repayment);
        loan.credit_score = new_score;
        ctx.accounts.borrower_profile.current_credit_score = new_score;
        
        // Check completion
        if loan.amount_repaid >= loan.principal {
            loan.status = LoanStatus::Repaid;
            ctx.accounts.borrower_profile.active_loans -= 1;
            ctx.accounts.borrower_profile.completed_loans += 1;
        }
        
        emit!(AutoRepayment {
            loan_id: loan.loan_id,
            revenue_amount,
            repayment_amount: actual_repayment,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        Ok(())
    }

    // Forgive remaining loan balance
    pub fn forgive_loan(
        ctx: Context<ForgiveLoan>,
        amount: Option<u64>, // None = forgive all
    ) -> Result<()> {
        let loan = &mut ctx.accounts.loan_account;
        
        require!(
            ctx.accounts.lender.key() == loan.lender,
            ErrorCode::Unauthorized
        );
        
        require!(loan.status == LoanStatus::Active, ErrorCode::LoanNotActive);
        
        let forgive_amount = amount.unwrap_or(loan.principal.saturating_sub(loan.amount_repaid));
        let old_status = loan.status;
        
        loan.amount_repaid += forgive_amount;
        
        if loan.amount_repaid >= loan.principal {
            loan.status = LoanStatus::Forgiven;
        }
        
        // Credit score impact (forgiveness is positive but less than repayment)
        loan.credit_score = std::cmp::min(loan.credit_score + 25, 850);
        
        emit!(LoanForgiven {
            loan_id: loan.loan_id,
            forgiven_amount: forgive_amount,
            new_status: loan.status,
            timestamp: Clock::get()?.unix_timestamp,
        });
        
        msg!("Loan {} forgiven: ${}", loan.loan_id, forgive_amount / 1_000_000);
        
        Ok(())
    }

    // Calculate credit score (internal function)
    fn calculate_credit_score(
        loan: &LoanAccount,
        profile: &BorrowerProfile,
        payment_amount: u64,
    ) -> u16 {
        let mut score = loan.credit_score;
        
        // Base increase based on payment size relative to principal
        let payment_ratio = (payment_amount as u128)
            .checked_mul(1000)
            .unwrap()
            .checked_div(loan.principal as u128)
            .unwrap() as u16;
        
        score += payment_ratio / 10; // 10% of principal = 1 point
        
        // Bonus for consecutive payments
        if loan.payments_made > 0 {
            let time_since_last = Clock::get().unwrap().unix_timestamp 
                - loan.last_payment_at.unwrap_or(0);
            
            if time_since_last < 86400 * 35 { // Within ~1 month
                score += 5; // Consistency bonus
            }
        }
        
        // History bonus
        score += profile.completed_loans * 10;
        
        // Cap at 850
        std::cmp::min(score, 850)
    }
}

// Account structures
#[derive(Accounts)]
pub struct InitializeProtocol<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(
        init,
        payer = authority,
        space = 8 + ProtocolConfig::SIZE,
        seeds = [b"protocol_config"],
        bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreateLoan<'info> {
    #[account(mut)]
    pub lender: Signer<'info>,
    
    /// CHECK: Borrower is validated in instruction
    pub borrower: AccountInfo<'info>,
    
    #[account(
        init,
        payer = lender,
        space = 8 + LoanAccount::SIZE,
        seeds = [
            b"loan",
            borrower.key().as_ref(),
            &protocol_config.total_loans_created.to_le_bytes()
        ],
        bump
    )]
    pub loan_account: Account<'info, LoanAccount>,
    
    #[account(
        init_if_needed,
        payer = lender,
        space = 8 + BorrowerProfile::SIZE,
        seeds = [b"borrower", borrower.key().as_ref()],
        bump
    )]
    pub borrower_profile: Account<'info, BorrowerProfile>,
    
    #[account(mut, seeds = [b"protocol_config"], bump = protocol_config.bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MakeRepayment<'info> {
    #[account(mut)]
    pub borrower: Signer<'info>,
    
    #[account(mut)]
    pub loan_account: Account<'info, LoanAccount>,
    
    #[account(mut)]
    pub borrower_profile: Account<'info, BorrowerProfile>,
    
    #[account(mut)]
    pub lender_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = borrower_token_account.owner == borrower.key()
    )]
    pub borrower_token_account: Account<'info, TokenAccount>,
    
    #[account(seeds = [b"protocol_config"], bump = protocol_config.bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    
    #[account(mut)]
    pub treasury_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct AutoRepayFromRevenue<'info> {
    #[account(mut)]
    pub company_authority: Signer<'info>,
    
    #[account(mut)]
    pub loan_account: Account<'info, LoanAccount>,
    
    #[account(mut)]
    pub borrower_profile: Account<'info, BorrowerProfile>,
    
    #[account(mut)]
    pub company_treasury: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub lender_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ForgiveLoan<'info> {
    #[account(mut)]
    pub lender: Signer<'info>,
    
    #[account(mut)]
    pub loan_account: Account<'info, LoanAccount>,
}

// Data structures
#[account]
pub struct ProtocolConfig {
    pub authority: Pubkey,
    pub protocol_fee_bps: u16,        // Basis points
    pub min_credit_score: u16,
    pub total_loans_created: u64,
    pub total_volume: u64,
    pub bump: u8,
}

impl ProtocolConfig {
    pub const SIZE: usize = 32 + 2 + 2 + 8 + 8 + 1;
}

#[account]
pub struct LoanAccount {
    pub loan_id: u64,
    pub borrower: Pubkey,
    pub lender: Pubkey,
    pub principal: u64,
    pub repayment_percentage: u16,    // Basis points
    pub term_months: Option<u16>,
    pub collateral_type: CollateralType,
    pub status: LoanStatus,
    pub amount_repaid: u64,
    pub payments_made: u32,
    pub credit_score: u16,
    pub created_at: i64,
    pub last_payment_at: Option<i64>,
    pub bump: u8,
}

impl LoanAccount {
    pub const SIZE: usize = 8 + 32 + 32 + 8 + 2 + 3 + 1 + 1 + 8 + 4 + 2 + 8 + 9 + 1;
}

#[account]
pub struct BorrowerProfile {
    pub owner: Pubkey,
    pub total_loans: u32,
    pub active_loans: u32,
    pub completed_loans: u32,
    pub total_borrowed: u64,
    pub total_repaid: u64,
    pub current_credit_score: u16,
    pub lifetime_credit_high: u16,
    pub bump: u8,
}

impl BorrowerProfile {
    pub const SIZE: usize = 32 + 4 + 4 + 4 + 8 + 8 + 2 + 2 + 1;
}

// Enums
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum LoanStatus {
    Active,
    Repaid,
    Defaulted,
    Forgiven,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum CollateralType {
    None,                 // Pure reputation
    TokenAccount,         // SPL tokens
    NFT,                  // NFT collateral
    RevenueShare,         // Future revenue rights
    Mixed,                // Combination
}

// Events
#[event]
pub struct LoanCreated {
    pub loan_id: u64,
    pub borrower: Pubkey,
    pub lender: Pubkey,
    pub principal: u64,
    pub repayment_percentage: u16,
    pub timestamp: i64,
}

#[event]
pub struct PaymentMade {
    pub loan_id: u64,
    pub amount: u64,
    pub fee: u64,
    pub old_credit_score: u16,
    pub new_credit_score: u16,
    pub remaining_principal: u64,
    pub timestamp: i64,
}

#[event]
pub struct LoanRepaid {
    pub loan_id: u64,
    pub borrower: Pubkey,
    pub total_repaid: u64,
    pub timestamp: i64,
}

#[event]
pub struct LoanForgiven {
    pub loan_id: u64,
    pub forgiven_amount: u64,
    pub new_status: LoanStatus,
    pub timestamp: i64,
}

#[event]
pub struct AutoRepayment {
    pub loan_id: u64,
    pub revenue_amount: u64,
    pub repayment_amount: u64,
    pub timestamp: i64,
}
lib
// Errors
#[error_code]
pub enum ErrorCode {
    #[msg("Invalid repayment percentage")]
    InvalidPercentage,
    #[msg("Principal amount too small")]
    PrincipalTooSmall,
    #[msg("Invalid payment amount")]
    InvalidAmount,
    #[msg("Loan is not active")]
    LoanNotActive,
    #[msg("Unauthorized")]
    Unauthorized,
}

