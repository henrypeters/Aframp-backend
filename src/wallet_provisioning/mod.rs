//! Wallet Creation & Stellar Account Provisioning (Issue #322).
//!
//! Implements the full end-to-end provisioning journey:
//! keypair generation guidance → account funding → activation detection →
//! cNGN trustline establishment → readiness verification.
//!
//! Every step is idempotent and resumable across sessions.

pub mod bip44;
pub mod handlers;
pub mod metrics;
pub mod models;
pub mod repository;
pub mod routes;
pub mod service;

pub use models::*;
pub use repository::ProvisioningRepository;
pub use routes::wallet_provisioning_routes;
pub use service::WalletProvisioningService;
