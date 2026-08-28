CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS plans (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL UNIQUE CHECK (char_length(trim(name)) BETWEEN 1 AND 64),
    monthly_credits bigint NOT NULL DEFAULT 0 CHECK (monthly_credits >= 0),
    rpm_limit integer NOT NULL DEFAULT 60 CHECK (rpm_limit > 0),
    tpm_limit integer NOT NULL DEFAULT 0 CHECK (tpm_limit >= 0),
    max_concurrency integer NOT NULL DEFAULT 2 CHECK (max_concurrency > 0),
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS users (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email text NOT NULL UNIQUE CHECK (char_length(email) BETWEEN 3 AND 320),
    password_hash text NOT NULL CHECK (char_length(password_hash) > 0),
    role text NOT NULL DEFAULT 'CUSTOMER' CHECK (role IN ('ADMIN', 'CUSTOMER')),
    enabled boolean NOT NULL DEFAULT true,
    plan_id uuid REFERENCES plans(id) ON DELETE SET NULL,
    credits_balance bigint NOT NULL DEFAULT 0 CHECK (credits_balance >= 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_users_enabled ON users(enabled);
CREATE INDEX IF NOT EXISTS idx_users_plan_id ON users(plan_id);

CREATE TABLE IF NOT EXISTS api_keys (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL DEFAULT '默认密钥' CHECK (char_length(trim(name)) BETWEEN 1 AND 64),
    key_prefix varchar(32) NOT NULL CHECK (char_length(key_prefix) BETWEEN 6 AND 32),
    key_hash varchar(128) NOT NULL UNIQUE CHECK (char_length(key_hash) > 0),
    enabled boolean NOT NULL DEFAULT true,
    expires_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,
    CONSTRAINT api_keys_expiry_valid CHECK (expires_at IS NULL OR expires_at > created_at)
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user_created ON api_keys(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_api_keys_user_enabled ON api_keys(user_id, enabled);
CREATE INDEX IF NOT EXISTS idx_api_keys_expiry ON api_keys(expires_at) WHERE enabled = true AND expires_at IS NOT NULL;

CREATE TABLE IF NOT EXISTS providers (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL UNIQUE CHECK (char_length(trim(name)) BETWEEN 1 AND 128),
    adapter text NOT NULL DEFAULT 'openai_compatible' CHECK (char_length(trim(adapter)) BETWEEN 1 AND 64),
    base_url text NOT NULL CHECK (base_url ~ '^https?://'),
    enabled boolean NOT NULL DEFAULT true,
    priority integer NOT NULL DEFAULT 100 CHECK (priority >= 0),
    weight integer NOT NULL DEFAULT 100 CHECK (weight >= 0),
    failure_count integer NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
    cooldown_until timestamptz,
    last_health_check_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_providers_available ON providers(enabled, priority, weight);

CREATE TABLE IF NOT EXISTS provider_credentials (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    provider_id uuid NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    label text NOT NULL CHECK (char_length(trim(label)) BETWEEN 1 AND 128),
    encrypted_secret text NOT NULL CHECK (char_length(encrypted_secret) > 0),
    secret_fingerprint varchar(32),
    status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'COOLDOWN', 'DISABLED', 'INVALID')),
    priority integer NOT NULL DEFAULT 100 CHECK (priority >= 0),
    weight integer NOT NULL DEFAULT 100 CHECK (weight >= 0),
    cooldown_until timestamptz,
    failure_count integer NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
    last_error_at timestamptz,
    last_used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider_id, label)
);

CREATE INDEX IF NOT EXISTS idx_provider_credentials_available
    ON provider_credentials(provider_id, status, priority, weight);
CREATE INDEX IF NOT EXISTS idx_provider_credentials_cooldown
    ON provider_credentials(cooldown_until)
    WHERE status = 'COOLDOWN';

CREATE TABLE IF NOT EXISTS models (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    model_name text NOT NULL UNIQUE CHECK (char_length(trim(model_name)) BETWEEN 1 AND 128),
    input_rate_micros bigint NOT NULL DEFAULT 1000 CHECK (input_rate_micros >= 0),
    output_rate_micros bigint NOT NULL DEFAULT 1000 CHECK (output_rate_micros >= 0),
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_models_enabled ON models(enabled, model_name);

CREATE TABLE IF NOT EXISTS model_routes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    model_id uuid NOT NULL REFERENCES models(id) ON DELETE CASCADE,
    provider_id uuid NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    upstream_model text NOT NULL CHECK (char_length(trim(upstream_model)) BETWEEN 1 AND 128),
    priority integer NOT NULL DEFAULT 100 CHECK (priority >= 0),
    weight integer NOT NULL DEFAULT 100 CHECK (weight >= 0),
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (model_id, provider_id)
);

CREATE INDEX IF NOT EXISTS idx_model_routes_lookup
    ON model_routes(model_id, enabled, priority, weight);
CREATE INDEX IF NOT EXISTS idx_model_routes_provider
    ON model_routes(provider_id, enabled);

CREATE TABLE IF NOT EXISTS usage_records (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id uuid NOT NULL UNIQUE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    api_key_id uuid NOT NULL REFERENCES api_keys(id) ON DELETE RESTRICT,
    provider_id uuid REFERENCES providers(id) ON DELETE SET NULL,
    credential_id uuid REFERENCES provider_credentials(id) ON DELETE SET NULL,
    model text NOT NULL CHECK (char_length(trim(model)) BETWEEN 1 AND 128),
    input_tokens bigint NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens bigint NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    credits bigint NOT NULL DEFAULT 0 CHECK (credits >= 0),
    stream boolean NOT NULL DEFAULT false,
    status text NOT NULL CHECK (char_length(trim(status)) BETWEEN 1 AND 64),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_usage_records_user_created ON usage_records(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_records_api_key_created ON usage_records(api_key_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_records_provider_created ON usage_records(provider_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_records_created_at ON usage_records(created_at DESC);

CREATE TABLE IF NOT EXISTS request_logs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id uuid NOT NULL UNIQUE,
    user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    api_key_id uuid REFERENCES api_keys(id) ON DELETE SET NULL,
    provider_id uuid REFERENCES providers(id) ON DELETE SET NULL,
    model text CHECK (model IS NULL OR char_length(trim(model)) BETWEEN 1 AND 128),
    status_code integer NOT NULL CHECK (status_code BETWEEN 100 AND 599),
    latency_ms bigint NOT NULL DEFAULT 0 CHECK (latency_ms >= 0),
    error_code text,
    stream boolean NOT NULL DEFAULT false,
    input_tokens bigint NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens bigint NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    credits bigint NOT NULL DEFAULT 0 CHECK (credits >= 0),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_request_logs_user_created ON request_logs(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_logs_api_key_created ON request_logs(api_key_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_logs_provider_created ON request_logs(provider_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_logs_status_created ON request_logs(status_code, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_logs_created_at ON request_logs(created_at DESC);

CREATE TABLE IF NOT EXISTS audit_logs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
    action text NOT NULL CHECK (char_length(trim(action)) BETWEEN 1 AND 128),
    resource_type text NOT NULL CHECK (char_length(trim(resource_type)) BETWEEN 1 AND 128),
    resource_id uuid,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_created ON audit_logs(actor_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_resource_created ON audit_logs(resource_type, resource_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs(created_at DESC);
