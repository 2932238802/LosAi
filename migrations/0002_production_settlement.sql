ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS stream boolean NOT NULL DEFAULT false;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS input_tokens bigint NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS output_tokens bigint NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN IF NOT EXISTS credits bigint NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_request_logs_user_created ON request_logs(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_records_request_id ON usage_records(request_id);
