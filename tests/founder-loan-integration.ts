// tests/founder-loan-integration.ts
import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { FounderLoan } from "../target/types/founder_loan_program";
import { expect } from "chai";

describe("founder-loan-program", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  
  const program = anchor.workspace.FounderLoan as Program<FounderLoan>;
  
  const lender = provider.wallet;
  const borrower = anchor.web3.Keypair.generate();
  
  it("Creates your founder loan", async () => {
    // Airdrop to borrower
    await provider.connection.requestAirdrop(
      borrower.publicKey, 
      2 * anchor.web3.LAMPORTS_PER_SOL
    );
    
    // Find PDA
    const [loanAccount] = anchor.web3.PublicKey.findProgramAddressSync(
      [
        Buffer.from("loan"),
        borrower.publicKey.toBuffer(),
        Buffer.from([0, 0, 0, 0, 0, 0, 0, 0]) // loan ID 0
      ],
      program.programId
    );
    
    // Create loan
    await program.methods
      .createLoan(
        new anchor.BN(50000000000), // $50,000 USDC
        2500, // 25%
        null, // No term
        { revenueShare: {} } // CollateralType enum
      )
      .accounts({
        lender: lender.publicKey,
        borrower: borrower.publicKey,
        loanAccount: loanAccount,
        // ... other accounts
      })
      .rpc();
    
    // Fetch and verify
    const loan = await program.account.loanAccount.fetch(loanAccount);
    
    console.log("Loan created:", {
      principal: loan.principal.toString(),
      repaymentPct: loan.repaymentPercentage,
      status: loan.status,
      creditScore: loan.creditScore
    });
    
    expect(loan.principal.toNumber()).to.equal(50000000000);
    expect(loan.repaymentPercentage).to.equal(2500);
    expect(loan.creditScore).to.equal(650); // Default starting score
  });
});

