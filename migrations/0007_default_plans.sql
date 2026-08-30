ALTER TABLE plans
    ADD COLUMN IF NOT EXISTS price_cents bigint NOT NULL DEFAULT 0 CHECK (price_cents >= 0),
    ADD COLUMN IF NOT EXISTS currency varchar(3) NOT NULL DEFAULT 'CNY',
    ADD COLUMN IF NOT EXISTS description text NOT NULL DEFAULT '';

INSERT INTO plans (name, monthly_credits, price_cents, currency, description, rpm_limit, tpm_limit, max_concurrency, monthly_request_limit, enabled)
VALUES
    ('GPT-5.6 基础版', 50, 4990, 'CNY', '适合个人日常使用', 30, 30000, 2, 1000, true),
    ('GPT-5.6 专业版', 150, 10890, 'CNY', '适合高频开发和长上下文使用', 60, 100000, 5, 5000, true)
ON CONFLICT (name) DO UPDATE SET
    price_cents = EXCLUDED.price_cents,
    currency = EXCLUDED.currency,
    description = EXCLUDED.description,
    monthly_credits = EXCLUDED.monthly_credits,
    rpm_limit = EXCLUDED.rpm_limit,
    tpm_limit = EXCLUDED.tpm_limit,
    max_concurrency = EXCLUDED.max_concurrency,
    monthly_request_limit = EXCLUDED.monthly_request_limit;
