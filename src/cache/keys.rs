//! Type-safe cache key builders

use std::fmt;

pub const VERSION: &str = "v1";

pub mod wallet {
    use super::*;

    pub const NAMESPACE: &str = "wallet";

    #[derive(Debug, Clone)]
    pub struct BalanceKey {
        pub address: String,
    }

    impl BalanceKey {
        pub fn new(address: impl Into<String>) -> Self {
            Self {
                address: address.into(),
            }
        }
    }

    impl fmt::Display for BalanceKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:balance:{}", VERSION, NAMESPACE, self.address)
        }
    }

    #[derive(Debug, Clone)]
    pub struct TrustlineKey {
        pub address: String,
    }

    impl TrustlineKey {
        pub fn new(address: impl Into<String>) -> Self {
            Self {
                address: address.into(),
            }
        }
    }

    impl fmt::Display for TrustlineKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:trustline:{}", VERSION, NAMESPACE, self.address)
        }
    }

    #[derive(Debug, Clone)]
    pub struct TransactionCountKey {
        pub address: String,
    }

    impl TransactionCountKey {
        pub fn new(address: impl Into<String>) -> Self {
            Self {
                address: address.into(),
            }
        }
    }

    impl fmt::Display for TransactionCountKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:tx_count:{}", VERSION, NAMESPACE, self.address)
        }
    }
}

pub mod exchange_rate {
    use super::*;

    pub const NAMESPACE: &str = "rate";

    #[derive(Debug, Clone)]
    pub struct CurrencyPairKey {
        pub from_currency: String,
        pub to_currency: String,
    }

    impl CurrencyPairKey {
        pub fn new(from_currency: impl Into<String>, to_currency: impl Into<String>) -> Self {
            Self {
                from_currency: from_currency.into(),
                to_currency: to_currency.into(),
            }
        }

        pub fn cngn_rate(to_currency: impl Into<String>) -> Self {
            Self::new("CNGN", to_currency)
        }
    }

    impl fmt::Display for CurrencyPairKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}:{}:{}:{}",
                VERSION, NAMESPACE, self.from_currency, self.to_currency
            )
        }
    }

    #[derive(Debug, Clone)]
    pub struct ConversionKey {
        pub amount: String,
        pub from_currency: String,
        pub to_currency: String,
    }

    impl ConversionKey {
        pub fn new(
            amount: impl Into<String>,
            from_currency: impl Into<String>,
            to_currency: impl Into<String>,
        ) -> Self {
            Self {
                amount: amount.into(),
                from_currency: from_currency.into(),
                to_currency: to_currency.into(),
            }
        }
    }

    impl fmt::Display for ConversionKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}:{}:convert:{}:{}:{}",
                VERSION, NAMESPACE, self.amount, self.from_currency, self.to_currency
            )
        }
    }
}

pub mod transaction {
    use super::*;

    pub const NAMESPACE: &str = "transaction";

    #[derive(Debug, Clone)]
    pub struct StatusKey {
        pub tx_hash: String,
    }

    impl StatusKey {
        pub fn new(tx_hash: impl Into<String>) -> Self {
            Self {
                tx_hash: tx_hash.into(),
            }
        }
    }

    impl fmt::Display for StatusKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:status:{}", VERSION, NAMESPACE, self.tx_hash)
        }
    }

    #[derive(Debug, Clone)]
    pub struct RecentKey {
        pub address: String,
    }

    impl RecentKey {
        pub fn new(address: impl Into<String>) -> Self {
            Self {
                address: address.into(),
            }
        }
    }

    impl fmt::Display for RecentKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:recent:{}", VERSION, NAMESPACE, self.address)
        }
    }
}

pub mod auth {
    use super::*;

    pub const NAMESPACE: &str = "auth";

    #[derive(Debug, Clone)]
    pub struct SessionKey {
        pub session_id: String,
    }

    impl SessionKey {
        pub fn new(session_id: impl Into<String>) -> Self {
            Self {
                session_id: session_id.into(),
            }
        }
    }

    impl fmt::Display for SessionKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:session:{}", VERSION, NAMESPACE, self.session_id)
        }
    }

    #[derive(Debug, Clone)]
    pub struct JwtKey {
        pub token_hash: String,
    }

    impl JwtKey {
        pub fn new(token_hash: impl Into<String>) -> Self {
            Self {
                token_hash: token_hash.into(),
            }
        }
    }

    impl fmt::Display for JwtKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:jwt:{}", VERSION, NAMESPACE, self.token_hash)
        }
    }

    #[derive(Debug, Clone)]
    pub struct RateLimitKey {
        pub identifier: String,
        pub action: String,
    }

    impl RateLimitKey {
        pub fn new(identifier: impl Into<String>, action: impl Into<String>) -> Self {
            Self {
                identifier: identifier.into(),
                action: action.into(),
            }
        }
    }

    impl fmt::Display for RateLimitKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}:{}:rate_limit:{}:{}",
                VERSION, NAMESPACE, self.identifier, self.action
            )
        }
    }
}

pub mod bill_payment {
    use super::*;

    pub const NAMESPACE: &str = "bill";

    #[derive(Debug, Clone)]
    pub struct ProviderKey {
        pub provider_id: String,
    }

    impl ProviderKey {
        pub fn new(provider_id: impl Into<String>) -> Self {
            Self {
                provider_id: provider_id.into(),
            }
        }
    }

    impl fmt::Display for ProviderKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:provider:{}", VERSION, NAMESPACE, self.provider_id)
        }
    }

    #[derive(Debug, Clone)]
    pub struct AvailabilityKey {
        pub country_code: String,
    }

    impl AvailabilityKey {
        pub fn new(country_code: impl Into<String>) -> Self {
            Self {
                country_code: country_code.into(),
            }
        }
    }

    impl fmt::Display for AvailabilityKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}:{}:available:{}",
                VERSION, NAMESPACE, self.country_code
            )
        }
    }
}

pub mod onramp {
    use super::*;

    pub const NAMESPACE: &str = "onramp";

    #[derive(Debug, Clone)]
    pub struct QuoteKey {
        pub quote_id: String,
    }

    impl QuoteKey {
        pub fn new(quote_id: impl Into<String>) -> Self {
            Self {
                quote_id: quote_id.into(),
            }
        }
    }

    impl fmt::Display for QuoteKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:quote:{}", VERSION, NAMESPACE, self.quote_id)
        }
    }

    #[derive(Debug, Clone)]
    pub struct StatusKey {
        pub tx_id: String,
    }

    impl StatusKey {
        pub fn new(tx_id: impl Into<String>) -> Self {
            Self {
                tx_id: tx_id.into(),
            }
        }
    }

    impl fmt::Display for StatusKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "api:{}:status:{}", NAMESPACE, self.tx_id)
        }
    }
}

pub mod fee {
    use super::*;

    pub const NAMESPACE: &str = "fee";

    #[derive(Debug, Clone)]
    pub struct StructureKey {
        pub fee_type: String,
    }

    impl StructureKey {
        pub fn new(fee_type: impl Into<String>) -> Self {
            Self {
                fee_type: fee_type.into(),
            }
        }
    }

    impl fmt::Display for StructureKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:structure:{}", VERSION, NAMESPACE, self.fee_type)
        }
    }

    /// API fees cache keys per spec
    pub const FEES_ALL: &str = "api:fees:all";

    pub fn fees_calculated(tx_type: &str, provider: &str, amount: &str) -> String {
        format!("api:fees:{}:{}:{}", tx_type, provider, amount)
    }

    pub fn fees_comparison(tx_type: &str, amount: &str) -> String {
        format!("api:fees:{}:all:{}", tx_type, amount)
    }
}

pub mod quote {
    use super::*;

    pub const NAMESPACE: &str = "quote";

    #[derive(Debug, Clone)]
    pub struct QuoteKey {
        pub quote_id: String,
    }

    impl QuoteKey {
        pub fn new(quote_id: impl Into<String>) -> Self {
            Self {
                quote_id: quote_id.into(),
            }
        }
    }

    impl fmt::Display for QuoteKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:{}", VERSION, NAMESPACE, self.quote_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_balance_key() {
        let key = wallet::BalanceKey::new("GA123456789");
        assert_eq!(key.to_string(), "v1:wallet:balance:GA123456789");
    }

    #[test]
    fn test_exchange_rate_key() {
        let key = exchange_rate::CurrencyPairKey::cngn_rate("USD");
        assert_eq!(key.to_string(), "v1:rate:CNGN:USD");
    }

    #[test]
    fn test_conversion_key() {
        let key = exchange_rate::ConversionKey::new("100.50", "CNGN", "USD");
        assert_eq!(key.to_string(), "v1:rate:convert:100.50:CNGN:USD");
    }

    #[test]
    fn test_session_key() {
        let key = auth::SessionKey::new("session_123");
        assert_eq!(key.to_string(), "v1:auth:session:session_123");
    }

    #[test]
    fn test_rate_limit_key() {
        let key = auth::RateLimitKey::new("user_123", "login");
        assert_eq!(key.to_string(), "v1:auth:rate_limit:user_123:login");
    }

    // Issue #459 — new namespaces
    #[test]
    fn test_user_profile_key() {
        let key = user::ProfileKey::new("uuid-abc");
        assert_eq!(key.to_string(), "v1:user:uuid-abc:profile");
    }

    #[test]
    fn test_user_onboarding_key() {
        let key = user::OnboardingKey::new("uuid-xyz");
        assert_eq!(key.to_string(), "v1:user:uuid-xyz:onboarding");
    }

    #[test]
    fn test_partner_config_key() {
        let key = partner::ConfigKey::new("partner-001");
        assert_eq!(key.to_string(), "v1:partner:partner-001:config");
    }

    #[test]
    fn test_partner_liquidity_key() {
        let key = partner::LiquidityKey::new("partner-002");
        assert_eq!(key.to_string(), "v1:partner:partner-002:liquidity");
    }

    #[test]
    fn test_namespace_pattern() {
        assert_eq!(namespace_pattern("rate"), "v1:rate:*");
        assert_eq!(namespace_pattern("user"), "v1:user:*");
        assert_eq!(namespace_pattern("partner"), "v1:partner:*");
    }

    #[test]
    fn test_user_pattern_isolation() {
        // Different users produce different patterns — no bleed
        let a = user::user_pattern("user-A");
        let b = user::user_pattern("user-B");
        assert_ne!(a, b);
        assert!(a.starts_with("v1:user:user-A:"));
        assert!(b.starts_with("v1:user:user-B:"));
    }

    #[test]
    fn test_key_collision_isolation() {
        // Profile and onboarding keys for same user must be distinct
        let profile = user::ProfileKey::new("uid-1").to_string();
        let onboarding = user::OnboardingKey::new("uid-1").to_string();
        assert_ne!(profile, onboarding);
    }
}

/// Returns the canonical SCAN pattern for a namespace.
/// Always uses the `v1:<ns>:*` format — never the legacy `cache:<ns>:v1:*` alias.
pub fn namespace_pattern(ns: &str) -> String {
    format!("v1:{}:*", ns)
}

// ---------------------------------------------------------------------------
// User profile, onboarding, and partner config cache keys (Issue #459)
// ---------------------------------------------------------------------------

pub mod user {
    use super::*;

    pub const NAMESPACE: &str = "user";

    /// User KYC / profile data: `v1:user:<user_id>:profile`
    #[derive(Debug, Clone)]
    pub struct ProfileKey {
        pub user_id: String,
    }

    impl ProfileKey {
        pub fn new(user_id: impl Into<String>) -> Self {
            Self { user_id: user_id.into() }
        }
    }

    impl fmt::Display for ProfileKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:{}:profile", VERSION, NAMESPACE, self.user_id)
        }
    }

    /// Progressive onboarding state: `v1:user:<user_id>:onboarding`
    #[derive(Debug, Clone)]
    pub struct OnboardingKey {
        pub user_id: String,
    }

    impl OnboardingKey {
        pub fn new(user_id: impl Into<String>) -> Self {
            Self { user_id: user_id.into() }
        }
    }

    impl fmt::Display for OnboardingKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:{}:onboarding", VERSION, NAMESPACE, self.user_id)
        }
    }

    /// All user keys for a given user_id (pattern for bulk purge)
    pub fn user_pattern(user_id: &str) -> String {
        format!("{}:{}:{}:*", VERSION, NAMESPACE, user_id)
    }
}

pub mod partner {
    use super::*;

    pub const NAMESPACE: &str = "partner";

    /// Partner API configuration: `v1:partner:<partner_id>:config`
    #[derive(Debug, Clone)]
    pub struct ConfigKey {
        pub partner_id: String,
    }

    impl ConfigKey {
        pub fn new(partner_id: impl Into<String>) -> Self {
            Self { partner_id: partner_id.into() }
        }
    }

    impl fmt::Display for ConfigKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:{}:config", VERSION, NAMESPACE, self.partner_id)
        }
    }

    /// Partner liquidity depth: `v1:partner:<partner_id>:liquidity`
    #[derive(Debug, Clone)]
    pub struct LiquidityKey {
        pub partner_id: String,
    }

    impl LiquidityKey {
        pub fn new(partner_id: impl Into<String>) -> Self {
            Self { partner_id: partner_id.into() }
        }
    }

    impl fmt::Display for LiquidityKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:{}:liquidity", VERSION, NAMESPACE, self.partner_id)
        }
    }
}

pub mod signing {
    use super::*;

    pub const NAMESPACE: &str = "sigkey";

    /// Cached derived signing key: `v1:sigkey:<key_id>`
    ///
    /// Stored in Redis with a short TTL to avoid re-deriving HKDF on every request.
    #[derive(Debug, Clone)]
    pub struct DerivedKeyCache {
        pub key_id: String,
    }

    impl DerivedKeyCache {
        pub fn new(key_id: impl Into<String>) -> Self {
            Self {
                key_id: key_id.into(),
            }
        }
    }

    impl fmt::Display for DerivedKeyCache {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{}:{}", VERSION, NAMESPACE, self.key_id)
        }
    }
}

pub mod replay {
    use super::*;

    pub const NAMESPACE: &str = "nonce";

    /// Namespaced nonce key: `v1:nonce:<consumer_id>:<nonce>`
    ///
    /// Stored in Redis with TTL = timestamp_window + safety_buffer.
    /// Presence of this key means the nonce has already been consumed.
    #[derive(Debug, Clone)]
    pub struct NonceKey {
        pub consumer_id: String,
        pub nonce: String,
    }

    impl NonceKey {
        pub fn new(consumer_id: impl Into<String>, nonce: impl Into<String>) -> Self {
            Self {
                consumer_id: consumer_id.into(),
                nonce: nonce.into(),
            }
        }
    }

    impl fmt::Display for NonceKey {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{}:{}:{}:{}",
                VERSION, NAMESPACE, self.consumer_id, self.nonce
            )
        }
    }
}
