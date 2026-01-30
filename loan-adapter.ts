// Ledger/src/loan-adapter.ts
import { Program } from '@coral-xyz/anchor';
import { Connection, PublicKey } from '@solana/web3.js';

interface LoanLedgerEntry {
  transactionType: 'LOAN_CREATED' | 'PAYMENT_RECEIVED' | 'LOAN_REPAID' | 'LOAN_FORGIVEN';
  loanId: number;
  amount: number;
  currency: 'USDC';
  debitAccount: string;
  creditAccount: string;
  metadata: {
    creditScoreBefore?: number;
    creditScoreAfter?: number;
    remainingPrincipal: number;
    paymentNumber?: number;
  };
  timestamp: number;
  signature: string;
}

export class LoanLedgerAdapter {
  constructor(
    private connection: Connection,
    private loanProgram: Program,
    private ledgerApi: string // Your ledger service endpoint
  ) {}

  /**
   * Listen for loan events and record to ledger
   */
  async startEventListener(): Promise<void> {
    this.connection.onLogs(this.loanProgram.programId, (logs) => {
      this.processLogs(logs);
    });
  }

  private async processLogs(logs: any): Promise<void> {
    for (const log of logs.logs) {
      if (log.includes('LoanCreated')) {
        await this.recordLoanCreated(logs.signature, logs.logs);
      } else if (log.includes('PaymentMade')) {
        await this.recordPayment(logs.signature, logs.logs);
      } else if (log.includes('LoanRepaid')) {
        await this.recordLoanRepaid(logs.signature, logs.logs);
      }
    }
  }

  private async recordPayment(signature: string, logs: string[]): Promise<void> {
    // Parse event data from logs
    const eventData = this.parseEventData(logs, 'PaymentMade');
    
    const entry: LoanLedgerEntry = {
      transactionType: 'PAYMENT_RECEIVED',
      loanId: eventData.loanId,
      amount: eventData.amount / 1_000_000, // Convert to USDC
      currency: 'USDC',
      debitAccount: 'CASH-USDC',
      creditAccount: 'LOAN_PAYABLE-FOUNDER',
      metadata: {
        creditScoreBefore: eventData.oldCreditScore,
        creditScoreAfter: eventData.newCreditScore,
        remainingPrincipal: eventData.remainingPrincipal / 1_000_000,
        paymentNumber: eventData.paymentNumber,
      },
      timestamp: Date.now(),
      signature,
    };

    // Post to ledger
    await this.postToLedger(entry);
    
    console.log(`Ledger entry created for payment: ${signature}`);
  }

  private async postToLedger(entry: LoanLedgerEntry): Promise<void> {
    const response = await fetch(`${this.ledgerApi}/entries`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(entry),
    });
    
    if (!response.ok) {
      throw new Error(`Failed to post to ledger: ${await response.text()}`);
    }
  }

  private parseEventData(logs: string[], eventName: string): any {
    // Parse Anchor event data from transaction logs
    // This is simplified - use proper Anchor event parsing in production
    const eventLog = logs.find(l => l.includes(eventName));
    if (!eventLog) return null;
    
    // Extract data from base64 encoded event
    // Implementation depends on your Anchor version
    return JSON.parse(Buffer.from(eventLog.split(':')[1], 'base64').toString());
  }

  /**
   * Generate financial reports
   */
  async generateLoanReport(startDate: Date, endDate: Date): Promise<{
    totalPayments: number;
    totalPrincipalRepaid: number;
    averageCreditScoreChange: number;
    outstandingBalance: number;
  }> {
    const entries = await this.fetchLedgerEntries(startDate, endDate);
    
    return {
      totalPayments: entries.filter(e => e.transactionType === 'PAYMENT_RECEIVED').length,
      totalPrincipalRepaid: entries
        .filter(e => e.transactionType === 'PAYMENT_RECEIVED')
        .reduce((sum, e) => sum + e.amount, 0),
      averageCreditScoreChange: this.calculateAverageScoreChange(entries),
      outstandingBalance: await this.calculateOutstandingBalance(),
    };
  }

  private async fetchLedgerEntries(start: Date, end: Date): Promise<LoanLedgerEntry[]> {
    const response = await fetch(
      `${this.ledgerApi}/entries?start=${start.toISOString()}&end=${end.toISOString()}`
    );
    return response.json();
  }
}

