//! Offramp transaction models and types
//!
//! Defines structures for the withdrawal flow (cNGN → NGN):
//! - Request types for initiation
//! - Response types for confirmation
//! - Transaction status and state management
//! - Bank details validation

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::BigDecimal;
use uuid::Uuid;

// ===== TRANSACTION STATUS ENUM =====

/// Offramp transaction status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfframpTransactionStatus {
    /// Initial state: Waiting for cNGN payment from user
    PendingPayment,
    /// cNGN received on system wallet, amount verified
    CngnReceived,
    /// Verifying payment amount matches quote
    VerifyingAmount,
    /// Processing withdrawal to user's bank account
    ProcessingWithdrawal,
    /// Transfer pending at payment provider
    TransferPending,
    /// Successfully completed
    Completed,
    /// Refund requested by user or system
    RefundInitiated,
    /// Refunding cNGN back to user wallet
    Refunding,
    /// Successfully refunded
    Refunded,
    /// Failed due to error
    Failed,
    /// Expired (no payment within 30 minutes)
    Expired,
}

impl OfframpTransactionStatus {
    /// Get string representation for database storage
    pub fn as_str(&self) -> &'static str {
        match self {
            OfframpTransactionStatus::PendingPayment => "pending_payment",
            OfframpTransactionStatus::CngnReceived => "cngn_received",
            OfframpTransactionStatus::VerifyingAmount => "verifying_amount",
            OfframpTransactionStatus::ProcessingWithdrawal => "processing_withdrawal",
            OfframpTransactionStatus::TransferPending => "transfer_pending",
            OfframpTransactionStatus::Completed => "completed",
            OfframpTransactionStatus::RefundInitiated => "refund_initiated",
            OfframpTransactionStatus::Refunding => "refunding",
            OfframpTransactionStatus::Refunded => "refunded",
            OfframpTransactionStatus::Failed => "failed",
            OfframpTransactionStatus::Expired => "expired",
        }
    }

    /// Parse from string (for database queries)
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending_payment" => Some(OfframpTransactionStatus::PendingPayment),
            "cngn_received" => Some(OfframpTransactionStatus::CngnReceived),
            "verifying_amount" => Some(OfframpTransactionStatus::VerifyingAmount),
            "processing_withdrawal" => Some(OfframpTransactionStatus::ProcessingWithdrawal),
            "transfer_pending" => Some(OfframpTransactionStatus::TransferPending),
            "completed" => Some(OfframpTransactionStatus::Completed),
            "refund_initiated" => Some(OfframpTransactionStatus::RefundInitiated),
            "refunding" => Some(OfframpTransactionStatus::Refunding),
            "refunded" => Some(OfframpTransactionStatus::Refunded),
            "failed" => Some(OfframpTransactionStatus::Failed),
            "expired" => Some(OfframpTransactionStatus::Expired),
            _ => None,
        }
    }

    /// Check if status represents a terminal state (no further transitions possible)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OfframpTransactionStatus::Completed
                | OfframpTransactionStatus::Refunded
                | OfframpTransactionStatus::Failed
                | OfframpTransactionStatus::Expired
        )
    }

    /// Check if status represents a success state
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            OfframpTransactionStatus::Completed | OfframpTransactionStatus::Refunded
        )
    }

    /// Check if status represents a failure state
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            OfframpTransactionStatus::Failed | OfframpTransactionStatus::Expired
        )
    }

    /// Validate state transition
    pub fn can_transition_to(&self, next: &OfframpTransactionStatus) -> bool {
        match (self, next) {
            // Normal successful flow
            (OfframpTransactionStatus::PendingPayment, OfframpTransactionStatus::CngnReceived) => {
                true
            }
            (OfframpTransactionStatus::CngnReceived, OfframpTransactionStatus::VerifyingAmount) => {
                true
            }
            (
                OfframpTransactionStatus::VerifyingAmount,
                OfframpTransactionStatus::ProcessingWithdrawal,
            ) => true,
            (
                OfframpTransactionStatus::ProcessingWithdrawal,
                OfframpTransactionStatus::TransferPending,
            ) => true,
            (OfframpTransactionStatus::TransferPending, OfframpTransactionStatus::Completed) => {
                true
            }

            // Failure/Expiry transitions
            (OfframpTransactionStatus::PendingPayment, OfframpTransactionStatus::Expired) => true,
            (_, OfframpTransactionStatus::RefundInitiated) => true, // Can initiate refund from most states
            (OfframpTransactionStatus::RefundInitiated, OfframpTransactionStatus::Refunding) => {
                true
            }
            (OfframpTransactionStatus::Refunding, OfframpTransactionStatus::Refunded) => true,

            // Failed can be set from certain states
            (OfframpTransactionStatus::CngnReceived, OfframpTransactionStatus::Failed) => true,
            (OfframpTransactionStatus::VerifyingAmount, OfframpTransactionStatus::Failed) => true,
            (OfframpTransactionStatus::ProcessingWithdrawal, OfframpTransactionStatus::Failed) => {
                true
            }
            (OfframpTransactionStatus::TransferPending, OfframpTransactionStatus::Failed) => true,
            (OfframpTransactionStatus::Refunding, OfframpTransactionStatus::Failed) => true,

            _ => false,
        }
    }
}

// ===== BANK DETAILS =====

/// Bank account details for withdrawal destination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankDetails {
    /// 3-digit Nigerian bank code (e.g., "044" for GTBank)
    pub bank_code: String,
    /// 10-digit account number
    pub account_number: String,
    /// Account holder name
    pub account_name: String,
    /// Bank name (resolved from code)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_name: Option<String>,
}

// ===== WITHDRAWAL TRANSACTION RECORD =====

/// Withdrawal transaction record (stored in database)
///
/// Maps to `transactions` table with type='offramp'
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalTransaction {
    /// Unique transaction identifier (UUID)
    pub transaction_id: Uuid,

    /// User's Stellar wallet address (cNGN sender)
    pub wallet_address: String,

    /// Transaction type: "offramp"
    pub transaction_type: String,

    /// Quote ID that this withdrawal is based on
    pub quote_id: Uuid,

    /// Amount of cNGN to receive from user
    pub cngn_amount: BigDecimal,

    /// Amount of NGN to send to user's bank
    pub ngn_amount: BigDecimal,

    /// Exchange rate used (cNGN to NGN)
    pub exchange_rate: BigDecimal,

    /// Total fees (NGN)
    pub total_fees: BigDecimal,

    /// Bank account details for withdrawal
    pub bank_details: BankDetails,

    /// Unique payment memo for Stellar transaction matching
    /// Format: "WD-{8_uppercase_hex_chars}" (e.g., "WD-9F8E7D6C")
    pub payment_memo: String,

    /// Current transaction status
    pub status: OfframpTransactionStatus,

    /// When this transaction was created
    pub created_at: DateTime<Utc>,

    /// When this transaction will expire (30 minutes from creation)
    pub expires_at: DateTime<Utc>,

    /// When the cNGN payment was received
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_received_at: Option<DateTime<Utc>>,

    /// Stellar transaction hash when payment received
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blockchain_tx_hash: Option<String>,

    /// Payment provider reference (from bank/provider)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_reference: Option<String>,

    /// Error message if transaction failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

impl WithdrawalTransaction {
    /// Check if transaction is still waiting for payment
    pub fn is_pending_payment(&self) -> bool {
        self.status == OfframpTransactionStatus::PendingPayment
    }

    /// Check if transaction is expired
    pub fn is_expired(&self) -> bool {
        self.expires_at < Utc::now()
    }

    /// Check if transaction is in progress
    pub fn is_processing(&self) -> bool {
        matches!(
            self.status,
            OfframpTransactionStatus::CngnReceived
                | OfframpTransactionStatus::VerifyingAmount
                | OfframpTransactionStatus::ProcessingWithdrawal
                | OfframpTransactionStatus::TransferPending
                | OfframpTransactionStatus::Refunding
        )
    }

    /// Check if transaction is complete
    pub fn is_complete(&self) -> bool {
        matches!(
            self.status,
            OfframpTransactionStatus::Completed | OfframpTransactionStatus::Refunded
        )
    }

    /// Get time remaining before expiry
    pub fn time_to_expiry(&self) -> Option<std::time::Duration> {
        let now = Utc::now();
        if now < self.expires_at {
            Some(std::time::Duration::from_secs(
                (self.expires_at - now).num_seconds() as u64,
            ))
        } else {
            None
        }
    }
}

// ===== TRANSACTION METADATA (JSONB in Database) =====

/// Metadata stored in JSONB format in transactions table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalMetadata {
    /// Quote ID for reference
    pub quote_id: Uuid,
    /// Payment memo for matching incoming payments
    pub payment_memo: String,
    /// Bank code (3 digits)
    pub bank_code: String,
    /// Bank account number (10 digits)
    pub account_number: String,
    /// Account holder name
    pub account_name: String,
    /// Bank name (resolved)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_name: Option<String>,
    /// Transaction type: "offramp"
    pub withdrawal_type: String,
    /// When this transaction expires (ISO 8601)
    pub expires_at: String,
    /// Additional context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

// ===== STATUS FLOW DOCUMENTATION =====

/// Status flow for offramp transactions
///
/// # Normal Successful Flow
/// ```
/// pending_payment
///   ↓ (cNGN payment received)
/// cngn_received
///   ↓ (amount verified)
/// verifying_amount
///   ↓ (processing withdrawal)
/// processing_withdrawal
///   ↓ (bank processing)
/// transfer_pending
///   ↓ (payment completed)
/// completed ✅
/// ```
///
/// # Expiration Flow
/// ```
/// pending_payment
///   ↓ (30 minutes elapsed, no payment)
/// expired ❌
/// ```
///
/// # Failure/Refund Flows
/// ```
/// pending_payment/cngn_received/processing_withdrawal
///   ↓ (error occurs or user cancels)
/// failed/refund_initiated
///   ↓ (refund process starts)
/// refunding
///   ↓ (refund sent to user wallet)
/// refunded ↩️
/// ```
///
/// # Possible Terminal States
/// - `completed`: Successfully withdrawn NGN to bank
/// - `refunded`: cNGN refunded to user wallet
/// - `failed`: Error during processing
/// - `expired`: No payment received within 30 minutes
///

// ===== DATABASE SCHEMA REFERENCE =====

/// Reference for the transactions table schema
///
/// ```sql
/// CREATE TABLE transactions (
///     transaction_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
///     wallet_address VARCHAR(200) NOT NULL,
///     type VARCHAR(20) NOT NULL,      -- 'offramp' or 'onramp'
///     from_currency VARCHAR(10),      -- 'cNGN' for offramp
///     to_currency VARCHAR(10),        -- 'NGN' for offramp
///     from_amount DECIMAL(18, 8),     -- cNGN amount
///     to_amount DECIMAL(18, 2),       -- NGN amount
///     cngn_amount DECIMAL(18, 8),     -- Same as from_amount for offramp
///     status VARCHAR(50) NOT NULL,    -- Current transaction status
///     payment_provider VARCHAR(100),  -- Bank/provider name
///     payment_reference VARCHAR(255), -- Provider transaction reference
///     blockchain_tx_hash VARCHAR(255),-- Stellar transaction hash
///     error_message TEXT,             -- Error details if failed
///     metadata JSONB NOT NULL,        -- Structured data: memo, quote_id, bank details, expires_at
///     created_at TIMESTAMP,
///     updated_at TIMESTAMP
/// );
///
/// -- Index on memo for fast payment matching
/// CREATE INDEX idx_transaction_memo
///   ON transactions USING GIN ((metadata->>'payment_memo'));
///
/// -- Index on wallet for transaction history
/// CREATE INDEX idx_transaction_wallet
///   ON transactions (wallet_address, created_at DESC);
///
/// -- Index on status for monitoring
/// CREATE INDEX idx_transaction_status
///   ON transactions (status, created_at DESC);
/// ```

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_string_conversion() {
        let status = OfframpTransactionStatus::PendingPayment;
        assert_eq!(status.as_str(), "pending_payment");
        assert_eq!(
            OfframpTransactionStatus::from_str("pending_payment"),
            Some(OfframpTransactionStatus::PendingPayment)
        );
    }

    #[test]
    fn test_status_terminal_states() {
        assert!(OfframpTransactionStatus::Completed.is_terminal());
        assert!(OfframpTransactionStatus::Refunded.is_terminal());
        assert!(OfframpTransactionStatus::Failed.is_terminal());
        assert!(OfframpTransactionStatus::Expired.is_terminal());
        assert!(!OfframpTransactionStatus::PendingPayment.is_terminal());
        assert!(!OfframpTransactionStatus::ProcessingWithdrawal.is_terminal());
    }

    #[test]
    fn test_status_success_states() {
        assert!(OfframpTransactionStatus::Completed.is_success());
        assert!(OfframpTransactionStatus::Refunded.is_success());
        assert!(!OfframpTransactionStatus::Failed.is_success());
        assert!(!OfframpTransactionStatus::Expired.is_success());
    }

    #[test]
    fn test_status_transitions() {
        assert!(OfframpTransactionStatus::PendingPayment
            .can_transition_to(&OfframpTransactionStatus::CngnReceived));
        assert!(OfframpTransactionStatus::CngnReceived
            .can_transition_to(&OfframpTransactionStatus::VerifyingAmount));
        assert!(OfframpTransactionStatus::VerifyingAmount
            .can_transition_to(&OfframpTransactionStatus::ProcessingWithdrawal));
        assert!(OfframpTransactionStatus::ProcessingWithdrawal
            .can_transition_to(&OfframpTransactionStatus::TransferPending));
        assert!(OfframpTransactionStatus::TransferPending
            .can_transition_to(&OfframpTransactionStatus::Completed));

        // Invalid transition
        assert!(!OfframpTransactionStatus::Completed
            .can_transition_to(&OfframpTransactionStatus::Failed));
    }

    #[test]
    fn test_status_expiry_transition() {
        assert!(OfframpTransactionStatus::PendingPayment
            .can_transition_to(&OfframpTransactionStatus::Expired));
    }

    #[test]
    fn test_refund_flow() {
        assert!(OfframpTransactionStatus::CngnReceived
            .can_transition_to(&OfframpTransactionStatus::RefundInitiated));
        assert!(OfframpTransactionStatus::RefundInitiated
            .can_transition_to(&OfframpTransactionStatus::Refunding));
        assert!(OfframpTransactionStatus::Refunding
            .can_transition_to(&OfframpTransactionStatus::Refunded));
    }

    #[test]
    fn test_failure_transitions() {
        assert!(OfframpTransactionStatus::CngnReceived
            .can_transition_to(&OfframpTransactionStatus::Failed));
        assert!(OfframpTransactionStatus::ProcessingWithdrawal
            .can_transition_to(&OfframpTransactionStatus::Failed));
    }
}
