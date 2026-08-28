ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE api_keys DROP CONSTRAINT IF EXISTS api_keys_name_length;
ALTER TABLE api_keys ADD CONSTRAINT api_keys_name_length CHECK (char_length(trim(name)) BETWEEN 1 AND 64) NOT VALID;
CREATE INDEX IF NOT EXISTS idx_api_keys_user_created ON api_keys(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_api_keys_user_enabled ON api_keys(user_id, enabled);
CREATE INDEX IF NOT EXISTS idx_api_keys_expiry ON api_keys(expires_at) WHERE enabled = true AND expires_at IS NOT NULL;

ALTER TABLE providers ADD COLUMN IF NOT EXISTS priority integer NOT NULL DEFAULT 100;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS weight integer NOT NULL DEFAULT 100;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS failure_count integer NOT NULL DEFAULT 0;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS cooldown_until timestamptz;
ALTER TABLE providers ADD COLUMN IF NOT EXISTS last_health_check_at timestamptz;
CREATE INDEX IF NOT EXISTS idx_providers_available ON providers(enabled, priority, weight);

ALTER TABLE provider_credentials ADD COLUMN IF NOT EXISTS secret_fingerprint varchar(32);
ALTER TABLE provider_credentials ADD COLUMN IF NOT EXISTS weight integer NOT NULL DEFAULT 100;
ALTER TABLE provider_credentials ADD COLUMN IF NOT EXISTS failure_count integer NOT NULL DEFAULT 0;
ALTER TABLE provider_credentials ADD COLUMN IF NOT EXISTS last_used_at timestamptz;
ALTER TABLE provider_credentials ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
CREATE INDEX IF NOT EXISTS idx_provider_credentials_available ON provider_credentials(provider_id, status, priority, weight);
CREATE INDEX IF NOT EXISTS idx_provider_credentials_cooldown ON provider_credentials(cooldown_until) WHERE status = 'COOLDOWN';

ALTER TABLE models ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE model_routes ADD COLUMN IF NOT EXISTS weight integer NOT NULL DEFAULT 100;
ALTER TABLE model_routes ADD COLUMN IF NOT EXISTS created_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE model_routes ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();
CREATE INDEX IF NOT EXISTS idx_models_enabled ON models(enabled, model_name);
CREATE INDEX IF NOT EXISTS idx_model_routes_lookup_v2 ON model_routes(model_id, enabled, priority, weight);
CREATE INDEX IF NOT EXISTS idx_model_routes_provider ON model_routes(provider_id, enabled);

CREATE INDEX IF NOT EXISTS idx_usage_records_api_key_created ON usage_records(api_key_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_records_provider_created ON usage_records(provider_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_logs_api_key_created ON request_logs(api_key_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_logs_provider_created ON request_logs(provider_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_request_logs_status_created ON request_logs(status_code, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_created ON audit_logs(actor_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_resource_created ON audit_logs(resource_type, resource_id, created_at DESC);
