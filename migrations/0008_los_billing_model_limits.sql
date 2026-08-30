-- Los 分计费、模型级限额和充值订单
ALTER TABLE models
    ADD COLUMN IF NOT EXISTS rpm_limit integer NOT NULL DEFAULT 30 CHECK (rpm_limit > 0),
    ADD COLUMN IF NOT EXISTS tpm_limit bigint NOT NULL DEFAULT 100000 CHECK (tpm_limit >= 0),
    ADD COLUMN IF NOT EXISTS max_concurrency integer NOT NULL DEFAULT 3 CHECK (max_concurrency > 0),
    ADD COLUMN IF NOT EXISTS monthly_request_limit bigint NOT NULL DEFAULT 5000 CHECK (monthly_request_limit >= 0);

CREATE TABLE IF NOT EXISTS recharge_offers (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL UNIQUE CHECK (char_length(trim(name)) BETWEEN 1 AND 128),
    amount_cents bigint NOT NULL CHECK (amount_cents > 0),
    base_los bigint NOT NULL CHECK (base_los > 0),
    bonus_los bigint NOT NULL DEFAULT 0 CHECK (bonus_los >= 0),
    description text NOT NULL DEFAULT '',
    enabled boolean NOT NULL DEFAULT true,
    sort_order integer NOT NULL DEFAULT 100,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS recharge_orders (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    offer_id uuid REFERENCES recharge_offers(id) ON DELETE SET NULL,
    amount_cents bigint NOT NULL CHECK (amount_cents > 0),
    base_los bigint NOT NULL CHECK (base_los > 0),
    bonus_los bigint NOT NULL DEFAULT 0 CHECK (bonus_los >= 0),
    total_los bigint NOT NULL CHECK (total_los > 0),
    status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING','APPROVED','REJECTED','CANCELLED')),
    note text NOT NULL DEFAULT '',
    reviewed_by uuid REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_recharge_orders_status_created ON recharge_orders(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_recharge_orders_user_created ON recharge_orders(user_id, created_at DESC);

INSERT INTO models(model_name, input_rate_micros, output_rate_micros, rpm_limit, tpm_limit, max_concurrency, monthly_request_limit, enabled)
VALUES
    ('gpt-5.6-sol', 1620, 9700, 30, 50000, 2, 5000, true),
    ('gpt-5.6-terra', 810, 4860, 60, 100000, 4, 10000, true),
    ('gpt-5.6-luna', 320, 1940, 120, 300000, 8, 30000, true)
ON CONFLICT (model_name) DO UPDATE SET
    input_rate_micros = EXCLUDED.input_rate_micros,
    output_rate_micros = EXCLUDED.output_rate_micros,
    rpm_limit = EXCLUDED.rpm_limit,
    tpm_limit = EXCLUDED.tpm_limit,
    max_concurrency = EXCLUDED.max_concurrency,
    monthly_request_limit = EXCLUDED.monthly_request_limit;

INSERT INTO recharge_offers(name, amount_cents, base_los, bonus_los, description, sort_order)
VALUES
    ('Los 分 10 元', 1000, 500, 0, '基础兑换：1 元 = 50 Los 分', 10),
    ('Los 分 50 元', 5000, 2500, 100, '小额优惠充值', 20),
    ('Los 分 100 元', 10000, 5000, 300, '热门充值档位', 30),
    ('Los 分 200 元', 20000, 10000, 1000, '优惠充值', 40),
    ('Los 分 500 元', 50000, 25000, 3000, '大额优惠充值', 50)
ON CONFLICT (name) DO UPDATE SET
    amount_cents = EXCLUDED.amount_cents,
    base_los = EXCLUDED.base_los,
    bonus_los = EXCLUDED.bonus_los,
    description = EXCLUDED.description,
    sort_order = EXCLUDED.sort_order;
