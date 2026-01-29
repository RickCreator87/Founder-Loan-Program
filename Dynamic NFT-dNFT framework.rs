// CPI call to mint/update loan NFT
pub fn update_loan_nft(
    ctx: Context<UpdateLoanNFT>,
    loan: &LoanAccount,
) -> Result<()> {
    let nft_program = ctx.accounts.nft_program.to_account_info();
    
    // Update metadata based on loan state
    let metadata = LoanNFTMetadata {
        principal: loan.principal,
        repaid: loan.amount_repaid,
        credit_score: loan.credit_score,
        status: format!("{:?}", loan.status),
        visual_tier: calculate_visual_tier(loan),
    };
    
    // Call your dNFT program
    dynamic_nft::cpi::update_metadata(
        CpiContext::new(nft_program, ctx.accounts.nft_accounts),
        metadata,
    )?;
    
    Ok(())
}

