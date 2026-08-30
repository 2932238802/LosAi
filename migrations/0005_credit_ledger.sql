CREATE TABLE IF NOT EXISTS credit_ledger (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    amount bigint NOT NULL CHECK (amount <> 0),
    balance_before bigint NOT NULL CHECK (balance_before >= 0),
    balance_after bigint NOT NULL CHECK (balance_after >= 0),
    entry_type text NOT NULL CHECK (entry_type IN ('ADMIN_TOP_UP', 'ADMIN_ADJUSTMENT', 'USAGE_DEBIT', 'REFUND')),
    description text NOT NULL DEFAULT '',
    request_id uuid REFERENCES request_logs(request_id) ON DELETE SET NULL,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_credit_ledger_usage_request
    ON credit_ledger(request_id) WHERE entry_type = 'USAGE_DEBIT';
CREATE INDEX IF NOT EXISTS idx_credit_ledger_user_created
    ON credit_ledger(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_credit_ledger_request_id
    ON credit_ledger(request_id);

ALTER TABLE usage_records ADD COLUMN IF NOT EXISTS usage_source text NOT NULL DEFAULT 'PROVIDER_REPORTED';
ALTER TABLE usage_records ADD COLUMN IF NOT EXISTS precharged_credits bigint NOT NULL DEFAULT 0;
ALTER TABLE usage_records DROP CONSTRAINT IF EXISTS usage_records_usage_source_check;
ALTER TABLE usage_records ADD CONSTRAINT usage_records_usage_source_check
    CHECK (usage_source IN ('PROVIDER_REPORTED', 'ESTIMATED'));
