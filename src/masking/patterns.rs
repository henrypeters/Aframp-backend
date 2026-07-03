//! Pattern-based scanner for sensitive data in unstructured log message strings.

use regex::Regex;
use std::sync::OnceLock;

/// Error type for pattern initialization failures.
#[derive(Debug, thiserror::Error)]
pub enum PatternError {
    #[error("Failed to compile regex pattern '{pattern}': {source}")]
    RegexCompilation {
        pattern: &'static str,
        source: regex::Error,
    },
}

/// Compile a regex pattern at initialization time.
/// 
/// # Panics
/// 
/// This function intentionally panics if the hardcoded regex pattern is invalid.
/// All patterns in this module are constant strings verified by tests. If a pattern
/// fails to compile, it indicates a programming error that must be fixed at development time.
#[inline]
fn compile_pattern(pattern: &'static str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| {
        panic!(
            "FATAL: Failed to compile hardcoded regex pattern '{}': {}. \
             This is a programming error that must be fixed.",
            pattern, e
        )
    })
}

fn re_jwt() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        compile_pattern(r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}")
    })
}

fn re_pem_private_key() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        compile_pattern(
            r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----[\s\S]*?-----END (?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
        )
    })
}

fn re_stellar_secret() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_pattern(r"\bS[A-Z2-7]{55}\b"))
}

fn re_credit_card() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        compile_pattern(
            r"\b(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13}|6(?:011|5[0-9]{2})[0-9]{12})\b",
        )
    })
}

fn re_api_key() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_pattern(r"(?i)(?:api[_-]?key|apikey)[=:\s]+[A-Za-z0-9_\-]{16,}"))
}

fn re_bvn() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_pattern(r"\bBVN[:\s]*[0-9]{11}\b"))
}

fn re_email() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_pattern(r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b"))
}

fn re_nin() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| compile_pattern(r"\b[0-9]{11}\b"))
}

struct Pattern {
    name: &'static str,
    placeholder: &'static str,
    get: fn() -> &'static Regex,
}

static PATTERNS: &[Pattern] = &[
    Pattern {
        name: "jwt",
        placeholder: "[JWT-REDACTED]",
        get: re_jwt,
    },
    Pattern {
        name: "pem_private_key",
        placeholder: "[PRIVKEY-REDACTED]",
        get: re_pem_private_key,
    },
    Pattern {
        name: "stellar_secret",
        placeholder: "[STELLAR-SECRET-REDACTED]",
        get: re_stellar_secret,
    },
    Pattern {
        name: "credit_card",
        placeholder: "[CARD-REDACTED]",
        get: re_credit_card,
    },
    Pattern {
        name: "api_key",
        placeholder: "[APIKEY-REDACTED]",
        get: re_api_key,
    },
    Pattern {
        name: "bvn",
        placeholder: "[BVN-REDACTED]",
        get: re_bvn,
    },
    Pattern {
        name: "email",
        placeholder: "[EMAIL-REDACTED]",
        get: re_email,
    },
    Pattern {
        name: "nin",
        placeholder: "[NIN-REDACTED]",
        get: re_nin,
    },
];

/// Scan a log message string and replace all sensitive patterns with placeholders.
/// Returns (sanitised_string, list_of_detected_pattern_names).
pub fn scan_and_redact(message: &str) -> (String, Vec<&'static str>) {
    let mut result = message.to_string();
    let mut detected = Vec::new();

    for p in PATTERNS {
        let re = (p.get)();
        if re.is_match(&result) {
            detected.push(p.name);
            result = re.replace_all(&result, p.placeholder).to_string();
        }
    }

    (result, detected)
}

/// Scan message and emit security alert if sensitive patterns are found.
/// Returns the sanitised message.
pub fn sanitise_log_message(message: &str) -> String {
    let (sanitised, detected) = scan_and_redact(message);
    for pattern_name in &detected {
        tracing::error!(
            pattern = pattern_name,
            "SECURITY ALERT: Sensitive data pattern detected in log message — remediation required"
        );
        crate::masking::metrics::record_masking_event(pattern_name, "log_message");
    }
    sanitised
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_redacted() {
        let msg = "token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let (out, detected) = scan_and_redact(msg);
        assert!(detected.contains(&"jwt"));
        assert!(!out.contains("eyJ"));
    }

    #[test]
    fn test_credit_card_redacted() {
        let msg = "card number is 4111111111111111 for payment";
        let (out, detected) = scan_and_redact(msg);
        assert!(detected.contains(&"credit_card"));
        assert!(!out.contains("4111111111111111"));
    }

    #[test]
    fn test_email_redacted() {
        let msg = "user email: john.doe@example.com logged in";
        let (out, detected) = scan_and_redact(msg);
        assert!(detected.contains(&"email"));
        assert!(!out.contains("john.doe@example.com"));
    }

    #[test]
    fn test_pem_key_redacted() {
        let msg = "key: -----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkq\n-----END PRIVATE KEY-----";
        let (out, detected) = scan_and_redact(msg);
        assert!(detected.contains(&"pem_private_key"));
        assert!(!out.contains("MIIEvQIBADANBgkq"));
    }

    #[test]
    fn test_stellar_secret_redacted() {
        // 56-char Stellar secret key starting with S
        let secret = "SCZANGBAYHTNYVSK3JYHXPJZXJZXJZXJZXJZXJZXJZXJZXJZXJZXJZXJ";
        let msg = format!("secret={}", secret);
        let (out, detected) = scan_and_redact(&msg);
        assert!(
            detected.contains(&"stellar_secret"),
            "detected: {:?}",
            detected
        );
        assert!(!out.contains("SCZANGBA"));
    }

    #[test]
    fn test_api_key_redacted() {
        let msg = "api_key=test_key_abcdefghijklmnopqrstuvwx";
        let (out, detected) = scan_and_redact(msg);
        assert!(detected.contains(&"api_key"));
        assert!(!out.contains("test_key_abcdefghijklmnopqrstuvwx"));
    }

    #[test]
    fn test_clean_message_unchanged() {
        let msg = "User completed onramp transaction of 5000 NGN";
        let (out, detected) = scan_and_redact(msg);
        assert!(detected.is_empty());
        assert_eq!(out, msg);
    }

    #[test]
    fn test_bvn_redacted() {
        let msg = "BVN: 12345678901 verified";
        let (out, detected) = scan_and_redact(msg);
        assert!(detected.contains(&"bvn"));
        assert!(!out.contains("12345678901"));
    }

    /// Verify that all hardcoded regex patterns compile successfully.
    /// This test ensures that pattern initialization won't panic at runtime.
    #[test]
    fn test_all_patterns_compile() {
        // Trigger compilation of all patterns
        let _ = re_jwt();
        let _ = re_pem_private_key();
        let _ = re_stellar_secret();
        let _ = re_credit_card();
        let _ = re_api_key();
        let _ = re_bvn();
        let _ = re_email();
        let _ = re_nin();
    }

    /// Verify compile_pattern provides clear error messages for invalid patterns.
    /// Note: This test demonstrates the panic behavior but cannot assert on it
    /// without additional test infrastructure. The panic is intentional and documented.
    #[test]
    #[should_panic(expected = "FATAL: Failed to compile hardcoded regex pattern")]
    fn test_compile_pattern_invalid_regex() {
        let _ = compile_pattern(r"[invalid(regex");
    }
}
