CREATE TABLE IF NOT EXISTS plan_requests (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plan_id uuid NOT NULL REFERENCES plans(id) ON DELETE RESTRICT,
    status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'APPROVED', 'REJECTED', 'CANCELLED')),
    note text NOT NULL DEFAULT '',
    reviewed_by uuid REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_plan_requests_one_pending
    ON plan_requests(user_id) WHERE status = 'PENDING';
CREATE INDEX IF NOT EXISTS idx_plan_requests_user_created
    ON plan_requests(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_plan_requests_status_created
    ON plan_requests(status, created_at DESC);
