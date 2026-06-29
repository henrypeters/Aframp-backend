-- Bootstrap consumer tables needed by early KYC and rate-limit migrations.
-- The fuller API-key scoping migration later in the timeline also creates
-- these tables with IF NOT EXISTS, so this file only establishes the minimum
-- schema required for a clean fresh-database bootstrap.

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

CREATE TABLE IF NOT EXISTS consumer_types (
    name        TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO consumer_types (name, description) VALUES
    ('mobile_client', 'Mobile application - end-user facing'),
    ('third_party_partner', 'External partner integrating the platform API'),
    ('backend_microservice', 'Internal microservice-to-microservice communication'),
    ('admin_dashboard', 'Admin dashboard with elevated privileges')
ON CONFLICT DO NOTHING;

CREATE TABLE IF NOT EXISTS consumers (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT NOT NULL,
    consumer_type TEXT NOT NULL REFERENCES consumer_types(name),
    environment   TEXT NOT NULL DEFAULT 'testnet' CHECK (environment IN ('testnet', 'mainnet')),
    is_active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_by    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_consumers_type ON consumers (consumer_type);
CREATE INDEX IF NOT EXISTS idx_consumers_env ON consumers (environment);
