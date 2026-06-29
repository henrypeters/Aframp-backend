# Issue #156 — Security Audit for Financial App

This document is the manual-review companion for automated scans.

## Scope
- OWASP Top 10 risk review
- Wallet-related dependency and integration review
- Dependency vulnerability management via Dependabot + Snyk + cargo-audit

## Automated controls (configured)
- Dependabot: `.github/dependabot.yml`
- Snyk/Cargo audit workflow: `.github/workflows/security-audit-156.yml`
- Existing broader security workflow: `.github/workflows/security-compliance.yml`

## Required secrets / vars
- `SNYK_TOKEN` (GitHub Actions secret) for Snyk job
- `SECURITY_SCAN_TARGET_URL` (GitHub Actions repository variable) for OWASP ZAP baseline scan target

---

## Manual Review Checklist (OWASP Top 10)

### A01 Broken Access Control
- [ ] Admin endpoints are protected by role middleware.
- [ ] No IDOR paths on transaction, wallet, compliance resources.
- [ ] Service-to-service/internal routes are non-public and authenticated.

### A02 Cryptographic Failures
- [ ] No hardcoded secrets/keys in code or repo history.
- [ ] TLS enforced in production ingress.
- [ ] Sensitive data encrypted at rest and in transit.

### A03 Injection
- [ ] SQL uses parameterized queries (`sqlx` bind variables).
- [ ] Any dynamic query construction is validated/allowlisted.
- [ ] Input validation exists for API payloads and query params.

### A04 Insecure Design
- [ ] Threat model exists for payment and settlement flows.
- [ ] Abuse cases reviewed (replay, double-spend-like sequencing, rate abuse).

### A05 Security Misconfiguration
- [ ] Debug features disabled in production.
- [ ] CORS and CSP/headers reviewed.
- [ ] Default credentials not present in environments.

### A06 Vulnerable and Outdated Components
- [ ] Dependabot PRs are enabled and triaged weekly.
- [ ] Snyk findings are reviewed and remediated for high/critical.
- [ ] `cargo audit` output tracked and exceptions documented.

### A07 Identification and Authentication Failures
- [ ] JWT/session expiration and refresh policy enforced.
- [ ] MFA paths verified for privileged operations.
- [ ] Brute force/rate-limiting controls in place.

### A08 Software and Data Integrity Failures
- [ ] CI only deploys from trusted branches/workflows.
- [ ] Lockfiles tracked and dependency provenance reviewed.
- [ ] Signature/checksum validation for external artifacts.

### A09 Security Logging and Monitoring Failures
- [ ] Failed auth, failed tx submission, and suspicious behavior are logged.
- [ ] Alerting exists for high-severity security findings.
- [ ] Forensic logs capture failure reason codes and timestamps.

### A10 Server-Side Request Forgery (SSRF)
- [ ] Outbound URL fetches are allowlisted where possible.
- [ ] Metadata/internal network endpoints blocked from user-controlled requests.

---

## Wallet Dependency Review (Manual)

Priority wallet/blockchain-related dependencies (from `Cargo.toml`) to review each release cycle:
- `stellar_sdk`
- `stellar-strkey`
- `stellar-xdr`
- `ed25519-dalek`
- `openssl`
- `aes-gcm`
- `rsa`
- `jsonwebtoken`

Checklist:
- [ ] Verify latest non-breaking patch/minor security updates.
- [ ] Review CVEs and advisories for crypto primitives and wallet libs.
- [ ] Confirm key-management and signing operations do not log secrets.
- [ ] Validate transaction signing and sequence handling tests still pass.

---

## Exit Criteria for Issue #156
- [ ] Dependabot active with PR generation for Cargo + GitHub Actions.
- [ ] Snyk workflow runs in CI when token is configured.
- [ ] Manual review checklist completed and attached to issue/PR.
