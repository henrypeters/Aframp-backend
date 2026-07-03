-- Developer Portal Schema
-- Creates tables for developer accounts, applications, credentials, and access management

-- Developer account status lookup table
CREATE TABLE IF NOT EXISTS developer_account_statuses (
    code TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE developer_account_statuses IS 'Lookup table for developer account statuses';
DO $$ BEGIN COMMENT ON COLUMN developer_account_statuses.code IS 'Machine-readable status code'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_account_statuses.description IS 'Human-readable status description'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

INSERT INTO developer_account_statuses (code, description) VALUES
    ('unverified', 'Account created but email not verified'),
    ('verified', 'Email verified, sandbox access granted'),
    ('identity_pending', 'Identity verification submitted'),
    ('identity_verified', 'Identity verification completed'),
    ('identity_rejected', 'Identity verification rejected'),
    ('suspended', 'Account suspended by admin'),
    ('active', 'Account fully active with production access')
ON CONFLICT (code) DO NOTHING;

-- Access tier lookup table
CREATE TABLE IF NOT EXISTS access_tiers (
    code TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    max_applications INTEGER NOT NULL DEFAULT 5,
    rate_limit_per_minute INTEGER NOT NULL DEFAULT 100,
    requires_identity_verification BOOLEAN NOT NULL DEFAULT FALSE,
    requires_business_agreement BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE access_tiers IS 'Access tiers with different privileges and limits';
DO $$ BEGIN COMMENT ON COLUMN access_tiers.code IS 'Machine-readable tier code'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN access_tiers.description IS 'Human-readable tier description'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN access_tiers.max_applications IS 'Maximum number of applications per developer'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN access_tiers.rate_limit_per_minute IS 'API rate limit per minute'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN access_tiers.requires_identity_verification IS 'Whether identity verification is required'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN access_tiers.requires_business_agreement IS 'Whether business agreement is required'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

INSERT INTO access_tiers (code, description, max_applications, rate_limit_per_minute, requires_identity_verification, requires_business_agreement) VALUES
    ('sandbox', 'Sandbox tier - testnet access only', 3, 50, FALSE, FALSE),
    ('standard', 'Standard tier - mainnet access with standard limits', 10, 1000, TRUE, FALSE),
    ('partner', 'Partner tier - mainnet access with elevated limits', 50, 10000, TRUE, TRUE)
ON CONFLICT (code) DO NOTHING;

-- Developer accounts table
CREATE TABLE IF NOT EXISTS developer_accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL UNIQUE,
    full_name TEXT NOT NULL,
    organisation TEXT,
    country TEXT NOT NULL,
    use_case_description TEXT NOT NULL,
    status_code TEXT NOT NULL DEFAULT 'unverified' REFERENCES developer_account_statuses(code),
    access_tier_code TEXT NOT NULL DEFAULT 'sandbox' REFERENCES access_tiers(code),
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    email_verification_token TEXT UNIQUE,
    email_verification_expires_at TIMESTAMPTZ,
    identity_verification_status TEXT DEFAULT 'unverified' CHECK (identity_verification_status IN ('unverified', 'pending', 'verified', 'rejected')),
    identity_verification_data JSONB DEFAULT '{}'::jsonb,
    identity_verified_at TIMESTAMPTZ,
    suspended_at TIMESTAMPTZ,
    suspension_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE developer_accounts IS 'Developer accounts for API access';
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.email IS 'Primary email address for the developer'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.full_name IS 'Full legal name of the developer'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.organisation IS 'Optional organisation name'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.country IS 'Country code (ISO 3166-1 alpha-2)'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.use_case_description IS 'Description of intended API usage'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.status_code IS 'Current account status'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.access_tier_code IS 'Current access tier'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.email_verified IS 'Whether email has been verified'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.email_verification_token IS 'Token for email verification'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.email_verification_expires_at IS 'Expiry time for email verification token'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.identity_verification_status IS 'Status of identity verification'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.identity_verification_data IS 'Identity verification documents and data'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.identity_verified_at IS 'Timestamp when identity was verified'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.suspended_at IS 'Timestamp when account was suspended'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_accounts.suspension_reason IS 'Reason for suspension'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

-- Developer applications table
CREATE TABLE IF NOT EXISTS developer_applications (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    developer_account_id UUID NOT NULL REFERENCES developer_accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    intended_use_case TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive', 'deleted')),
    sandbox_wallet_address VARCHAR(255),
    sandbox_wallet_secret TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE developer_applications IS 'Applications registered by developers';
DO $$ BEGIN COMMENT ON COLUMN developer_applications.developer_account_id IS 'Reference to developer account'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_applications.name IS 'Application name'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_applications.description IS 'Application description'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_applications.intended_use_case IS 'Intended use case for this application'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_applications.status IS 'Application status'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_applications.sandbox_wallet_address IS 'Testnet wallet address for sandbox'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN developer_applications.sandbox_wallet_secret IS 'Testnet wallet secret for sandbox'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

-- API keys table
CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id UUID NOT NULL REFERENCES developer_applications(id) ON DELETE CASCADE,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    key_name TEXT NOT NULL,
    environment TEXT NOT NULL DEFAULT 'sandbox' CHECK (environment IN ('sandbox', 'production')),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked', 'expired')),
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    usage_count INTEGER NOT NULL DEFAULT 0,
    rate_limit_per_minute INTEGER NOT NULL DEFAULT 100,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (key_prefix, key_hash)
);


-- Add developer portal columns to api_keys if they don't exist yet
ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS application_id UUID REFERENCES developer_applications(id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS key_name TEXT,
    ADD COLUMN IF NOT EXISTS environment TEXT NOT NULL DEFAULT 'sandbox',
    ADD COLUMN IF NOT EXISTS usage_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS rate_limit_per_minute INTEGER NOT NULL DEFAULT 100;

COMMENT ON TABLE api_keys IS 'API keys for developer applications';
DO $$ BEGIN COMMENT ON COLUMN api_keys.application_id IS 'Reference to application'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.key_prefix IS 'Public prefix of the API key'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.key_hash IS 'Hashed version of the full API key'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.key_name IS 'Human-readable name for the key'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.environment IS 'Environment (sandbox or production)'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.status IS 'Key status'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.expires_at IS 'Key expiry time'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.last_used_at IS 'Timestamp of last usage'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.usage_count IS 'Total usage count'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN api_keys.rate_limit_per_minute IS 'Rate limit per minute for this key'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

-- OAuth clients table
CREATE TABLE IF NOT EXISTS oauth_clients (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id UUID NOT NULL REFERENCES developer_applications(id) ON DELETE CASCADE,
    client_id TEXT NOT NULL UNIQUE,
    client_secret_hash TEXT NOT NULL,
    client_name TEXT NOT NULL,
    environment TEXT NOT NULL DEFAULT 'sandbox' CHECK (environment IN ('sandbox', 'production')),
    redirect_uris TEXT[] NOT NULL DEFAULT '{}',
    scopes TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked', 'expired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Add developer portal columns to oauth_clients if missing
ALTER TABLE oauth_clients
    ADD COLUMN IF NOT EXISTS application_id UUID REFERENCES developer_applications(id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS developer_account_id UUID REFERENCES developer_accounts(id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS environment TEXT NOT NULL DEFAULT 'sandbox';

COMMENT ON TABLE oauth_clients IS 'OAuth clients for developer applications';
DO $$ BEGIN COMMENT ON COLUMN oauth_clients.application_id IS 'Reference to application'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN oauth_clients.client_id IS 'OAuth client ID'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN oauth_clients.client_secret_hash IS 'Hashed client secret'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN oauth_clients.client_name IS 'Human-readable client name'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN oauth_clients.environment IS 'Environment (sandbox or production)'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN oauth_clients.redirect_uris IS 'Allowed redirect URIs'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN oauth_clients.scopes IS 'Granted scopes'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN oauth_clients.status IS 'Client status'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

-- Webhook configurations table
CREATE TABLE IF NOT EXISTS webhook_configurations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id UUID NOT NULL REFERENCES developer_applications(id) ON DELETE CASCADE,
    webhook_url TEXT NOT NULL,
    secret_token TEXT,
    events TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive', 'deleted')),
    delivery_success_rate NUMERIC(5,2) DEFAULT 0.00,
    average_delivery_latency INTEGER DEFAULT 0, -- in milliseconds
    failed_delivery_count INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE webhook_configurations IS 'Webhook configurations for applications';
DO $$ BEGIN COMMENT ON COLUMN webhook_configurations.application_id IS 'Reference to application'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_configurations.webhook_url IS 'Webhook endpoint URL'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_configurations.secret_token IS 'Secret for webhook signature verification'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_configurations.events IS 'Events to trigger webhooks'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_configurations.status IS 'Webhook status'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_configurations.delivery_success_rate IS 'Success rate percentage'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_configurations.average_delivery_latency IS 'Average delivery latency in ms'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_configurations.failed_delivery_count IS 'Count of failed deliveries'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

-- Production access requests table
CREATE TABLE IF NOT EXISTS production_access_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id UUID NOT NULL REFERENCES developer_applications(id) ON DELETE CASCADE,
    developer_account_id UUID NOT NULL REFERENCES developer_accounts(id) ON DELETE CASCADE,
    production_use_case TEXT NOT NULL,
    expected_transaction_volume TEXT NOT NULL,
    supported_countries TEXT[] NOT NULL DEFAULT '{}',
    business_details JSONB DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected')),
    reviewed_by_admin_id UUID,
    review_notes TEXT,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE production_access_requests IS 'Production access requests for applications';
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.application_id IS 'Reference to application'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.developer_account_id IS 'Reference to developer account'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.production_use_case IS 'Detailed production use case'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.expected_transaction_volume IS 'Expected monthly transaction volume'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.supported_countries IS 'List of supported countries'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.business_details IS 'Additional business information'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.status IS 'Request status'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.reviewed_by_admin_id IS 'Admin who reviewed the request'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.review_notes IS 'Admin review notes'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN production_access_requests.reviewed_at IS 'Timestamp of review'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

-- Usage statistics table
CREATE TABLE IF NOT EXISTS usage_statistics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id UUID NOT NULL REFERENCES developer_applications(id) ON DELETE CASCADE,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    endpoint TEXT NOT NULL,
    method TEXT NOT NULL,
    status_code INTEGER NOT NULL,
    response_time_ms INTEGER,
    request_size_bytes INTEGER,
    response_size_bytes INTEGER,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT now(),
    environment TEXT NOT NULL DEFAULT 'sandbox' CHECK (environment IN ('sandbox', 'production'))
);

COMMENT ON TABLE usage_statistics IS 'API usage statistics for monitoring and analytics';
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.application_id IS 'Reference to application'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.api_key_id IS 'Reference to API key used'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.endpoint IS 'API endpoint called'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.method IS 'HTTP method used'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.status_code IS 'HTTP response status code'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.response_time_ms IS 'Response time in milliseconds'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.request_size_bytes IS 'Request size in bytes'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.response_size_bytes IS 'Response size in bytes'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.timestamp IS 'Timestamp of the request'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN usage_statistics.environment IS 'Environment (sandbox or production)'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

-- Webhook delivery logs table
CREATE TABLE IF NOT EXISTS webhook_delivery_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    webhook_configuration_id UUID NOT NULL REFERENCES webhook_configurations(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    delivery_url TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'delivered', 'failed', 'retrying')),
    http_status_code INTEGER,
    response_body TEXT,
    delivery_attempts INTEGER NOT NULL DEFAULT 1,
    next_retry_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE webhook_delivery_logs IS 'Logs for webhook delivery attempts';
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.webhook_configuration_id IS 'Reference to webhook configuration'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.event_type IS 'Type of event being delivered'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.payload IS 'Event payload'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.delivery_url IS 'URL where webhook was delivered'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.status IS 'Delivery status'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.http_status_code IS 'HTTP status code from webhook endpoint'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.response_body IS 'Response body from webhook endpoint'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.delivery_attempts IS 'Number of delivery attempts'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.next_retry_at IS 'Timestamp for next retry attempt'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.delivered_at IS 'Timestamp when webhook was delivered'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;
DO $$ BEGIN COMMENT ON COLUMN webhook_delivery_logs.error_message IS 'Error message if delivery failed'; EXCEPTION WHEN undefined_column OR undefined_table THEN NULL; END $$;

-- Triggers for updated_at
DROP TRIGGER IF EXISTS set_updated_at_developer_account_statuses ON developer_account_statuses;
CREATE TRIGGER set_updated_at_developer_account_statuses
    BEFORE UPDATE ON developer_account_statuses
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS set_updated_at_access_tiers ON access_tiers;
CREATE TRIGGER set_updated_at_access_tiers
    BEFORE UPDATE ON access_tiers
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS set_updated_at_developer_accounts ON developer_accounts;
CREATE TRIGGER set_updated_at_developer_accounts
    BEFORE UPDATE ON developer_accounts
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS set_updated_at_developer_applications ON developer_applications;
CREATE TRIGGER set_updated_at_developer_applications
    BEFORE UPDATE ON developer_applications
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS set_updated_at_api_keys ON api_keys;
CREATE TRIGGER set_updated_at_api_keys
    BEFORE UPDATE ON api_keys
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS set_updated_at_oauth_clients ON oauth_clients;
CREATE TRIGGER set_updated_at_oauth_clients
    BEFORE UPDATE ON oauth_clients
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS set_updated_at_webhook_configurations ON webhook_configurations;
CREATE TRIGGER set_updated_at_webhook_configurations
    BEFORE UPDATE ON webhook_configurations
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS set_updated_at_production_access_requests ON production_access_requests;
CREATE TRIGGER set_updated_at_production_access_requests
    BEFORE UPDATE ON production_access_requests
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

DROP TRIGGER IF EXISTS set_updated_at_webhook_delivery_logs ON webhook_delivery_logs;
CREATE TRIGGER set_updated_at_webhook_delivery_logs
    BEFORE UPDATE ON webhook_delivery_logs
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_developer_accounts_email ON developer_accounts(email);
CREATE INDEX IF NOT EXISTS idx_developer_accounts_status ON developer_accounts(status_code);
CREATE INDEX IF NOT EXISTS idx_developer_accounts_tier ON developer_accounts(access_tier_code);
CREATE INDEX IF NOT EXISTS idx_developer_applications_developer_id ON developer_applications(developer_account_id);
CREATE INDEX IF NOT EXISTS idx_developer_applications_status ON developer_applications(status);
CREATE INDEX IF NOT EXISTS idx_api_keys_application_id ON api_keys(application_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_environment ON api_keys(environment);
CREATE INDEX IF NOT EXISTS idx_api_keys_status ON api_keys(status);
CREATE INDEX IF NOT EXISTS idx_oauth_clients_application_id ON oauth_clients(application_id);
CREATE INDEX IF NOT EXISTS idx_oauth_clients_environment ON oauth_clients(environment);
CREATE INDEX IF NOT EXISTS idx_webhook_configurations_application_id ON webhook_configurations(application_id);
CREATE INDEX IF NOT EXISTS idx_production_access_requests_application_id ON production_access_requests(application_id);
CREATE INDEX IF NOT EXISTS idx_production_access_requests_status ON production_access_requests(status);
CREATE INDEX IF NOT EXISTS idx_usage_statistics_application_id ON usage_statistics(application_id);
CREATE INDEX IF NOT EXISTS idx_usage_statistics_timestamp ON usage_statistics(timestamp);
CREATE INDEX IF NOT EXISTS idx_usage_statistics_environment ON usage_statistics(environment);
CREATE INDEX IF NOT EXISTS idx_webhook_delivery_logs_webhook_id ON webhook_delivery_logs(webhook_configuration_id);
CREATE INDEX IF NOT EXISTS idx_webhook_delivery_logs_status ON webhook_delivery_logs(status);
CREATE INDEX IF NOT EXISTS idx_webhook_delivery_logs_next_retry ON webhook_delivery_logs(next_retry_at) WHERE status = 'retrying';
