// gitdigital-shared/src/ecosystem-integration.ts
// This ties all 6 organizations together

import { LoanRepaymentIntegration } from '../../Checkout-core/src/loan-integration';
import { LoanLedgerAdapter } from '../../Ledger/src/loan-adapter';
import { DynamicNFTService } from '../../Dynamic-NFT-dNFT-Framework/src/service';
import { ReputationNetwork } from '../../Reputation-Network/src/client';
import { FounderLoanProgram } from '../../founder-loan-program/src/client';

export class GitDigitalEcosystem {
  private loanIntegration: LoanRepaymentIntegration;
  private ledgerAdapter: LoanLedgerAdapter;
  private nftService: DynamicNFTService;
  private reputation: ReputationNetwork;
  
  constructor(config: {
    solanaRpc: string;
    founderLoanProgramId: string;
    loanAccountAddress: string;
    ledgerApi: string;
    reputationApi: string;
  }) {
    // Initialize all components
    this.loanIntegration = new LoanRepaymentIntegration(
      config.solanaRpc,
      config.founderLoanProgramId,
      config.loanAccountAddress
    );
    
    this.ledgerAdapter = new LoanLedgerAdapter(
      config.solanaRpc,
      this.loanIntegration.program,
      config.ledgerApi
    );
    
    this.nftService = new DynamicNFTService(config.solanaRpc);
    this.reputation = new ReputationNetwork(config.reputationApi);
  }

  /**
   * Initialize the complete loop
   */
  async initialize(): Promise<void> {
    // Start listening for blockchain events
    await this.ledgerAdapter.startEventListener();
    
    // Sync NFT metadata with current loan state
    await this.syncNFTMetadata();
    
    // Update reputation network
    await this.syncReputation();
    
    console.log('✅ GitDigital Ecosystem initialized');
    console.log('   - Loan auto-repayment: Active');
    console.log('   - Ledger recording: Active');
    console.log('   - NFT evolution: Active');
    console.log('   - Reputation sync: Active');
  }

  /**
   * Process a customer payment through the entire ecosystem
   */
  async processPayment(payment: CustomerPayment): Promise<{
    loanRepayment: number;
    creditScoreChange: number;
    nftUpdated: boolean;
    ledgerEntry: string;
  }> {
    // 1. Process payment with auto-repayment
    const result = await this.loanIntegration.processRevenueWithLoanSplit(
      payment.amount,
      payment.treasury
    );

    // 2. Update NFT (triggered by event listener, but verify)
    const nftUpdated = await this.nftService.refreshLoanNFT(
      this.loanIntegration.loanAccount
    );

    // 3. Ledger entry created automatically by listener
    const ledgerEntry = await this.waitForLedgerEntry(result.txSignature);

    // 4. Update reputation
    const newScore = await this.reputation.updateScore({
      entity: payment.company,
      paymentAmount: result.loanRepayment,
      loanStatus: await this.loanIntegration.getLoanStatus()
    });

    return {
      loanRepayment: result.loanRepayment,
      creditScoreChange: newScore.change,
      nftUpdated,
      ledgerEntry: ledgerEntry.id
    };
  }

  private async syncNFTMetadata(): Promise<void> {
    const loanStatus = await this.loanIntegration.getLoanStatus();
    await this.nftService.updateMetadata({
      creditScore: loanStatus.creditScore,
      repaymentProgress: loanStatus.repaid / loanStatus.principal
    });
  }

  private async syncReputation(): Promise<void> {
    const profile = await this.loanIntegration.getBorrowerProfile();
    await this.reputation.syncProfile({
      totalBorrowed: profile.totalBorrowed,
      totalRepaid: profile.totalRepaid,
      currentScore: profile.currentCreditScore
    });
  }
}

// Usage:
const ecosystem = new GitDigitalEcosystem({
  solanaRpc: 'https://api.mainnet-beta.solana.com',
  founderLoanProgramId: 'YOUR_PROGRAM_ID',
  loanAccountAddress: 'YOUR_LOAN_ACCOUNT',
  ledgerApi: 'https://ledger.gitdigital.io',
  reputationApi: 'https://reputation.gitdigital.io'
});

await ecosystem.initialize();

