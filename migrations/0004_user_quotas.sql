ALTER TABLE plans
    ADD COLUMN IF NOT EXISTS monthly_request_limit bigint NOT NULL DEFAULT 0
        CHECK (monthly_request_limit >= 0);

COMMENT ON COLUMN plans.monthly_request_limit IS '每月请求次数；0 表示不限';
