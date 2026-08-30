# LosToken Credits Ledger、Usage Settlement 与前端模块化设计

日期：2026-08-29
状态：待用户审阅

## 1. 背景与目标

当前 Gateway 的 `usage_records` 只在 `billing::settle` 成功时写入，`request_logs` 也与结算事务耦合，导致普通用户调用后可能看不到使用量或请求日志。Credits 目前主要是 `users.credits_balance`，缺少管理员充值流水；前端所有登录、普通用户页面、管理员页面、API 请求和弹窗集中在 `App.vue`，已不利于维护。

本次目标：

1. 支持管理员手动给普通用户充值任意数量 Credits；不在系统内固定人民币兑换比例。
2. 所有余额变更写入不可变的 Credits 账本。
3. API 请求在调用上游前进行安全预扣；完成后按 Provider usage 结算；没有 usage 时使用预扣金额作为最终消费。
4. 认证成功后的请求，无论成功、限流、余额不足、模型不可用、上游失败或超时，都有最终请求日志。
5. 普通用户能看到自己的余额、消费流水、usage records 和 request logs。
6. 将 `App.vue` 拆为 API、store/composable、layout、view 和通用组件，`App.vue` 只保留应用入口。

## 2. 业务规则

### Credits

- `credits_balance` 是用户当前可用余额，不能为负数。
- 管理员充值使用正数金额；管理员调整可使用正数或负数，但结果不能小于零。
- API 消费使用负数账本条目 `USAGE_DEBIT`。
- 上游失败且未实际完成调用：不产生消费扣款；保留失败请求日志。
- 上游成功但没有 usage：使用请求预扣金额作为最终消费，并标记 `usage_source=ESTIMATED`。
- 上游返回 usage：按整数倍率计算实际 Credits，并退回预扣与实际消费之间的差额。
- 余额扣减必须使用数据库原子条件，禁止并发请求将余额扣成负数。

### 预扣

预扣金额为：

```text
estimated_input_tokens * input_rate_micros
+ requested_max_tokens * output_rate_micros
```

输入 Token 使用服务端确定性估算；`max_tokens` 缺失时使用配置的默认输出上限；预扣金额设置单请求上限。若预扣金额超过余额，请求在调用上游前拒绝。实现应使用余额更新条件或余额冻结字段，避免多个并发请求重复消费同一余额。

## 3. 数据库设计

新增 migration：`0005_credit_ledger.sql`。

新增表 `credit_ledger`：

- `id uuid primary key`
- `user_id uuid not null references users(id) on delete restrict`
- `amount bigint not null`
- `balance_before bigint not null`
- `balance_after bigint not null`
- `entry_type text not null`：`ADMIN_TOP_UP`、`ADMIN_ADJUSTMENT`、`USAGE_DEBIT`、`REFUND`
- `description text not null default ''`
- `request_id uuid null`
- `created_by uuid null references users(id) on delete set null`
- `created_at timestamptz not null default now()`

建立 `(user_id, created_at desc)`、`(request_id)` 索引，并对 `USAGE_DEBIT` 的 request_id 做唯一约束/幂等处理。

在 `usage_records` 增加：

- `usage_source text not null default 'PROVIDER_REPORTED'`，值为 `PROVIDER_REPORTED` 或 `ESTIMATED`
- 可选的 `precharged_credits bigint not null default 0`，用于审计预扣和实际结算

在 `request_logs` 增加或统一使用：

- `finished_at` 或等价最终状态时间字段
- 现有 `status_code`、`error_code`、`input_tokens`、`output_tokens`、`credits` 在请求结束时更新

不删除既有字段，迁移必须幂等并由 SQLx 启动迁移自动执行。

## 4. 后端接口

### 管理员

```http
POST /admin/users/{user_id}/credits
GET  /admin/users/{user_id}/credits/ledger
```

充值请求：

```json
{
  "amount": 100000,
  "description": "QQ群转账100元"
}
```

充值和账本写入必须处于同一数据库事务，并写入审计日志。

### 普通用户

```http
GET /user/credits
GET /user/credits/ledger?page=1&page_size=20
```

只允许根据 JWT subject 查询本人数据。

现有：

```http
GET /user/usage
GET /user/request-logs
```

继续保留分页响应：

```json
{
  "data": [],
  "page": 1,
  "page_size": 20,
  "total": 0,
  "total_pages": 0
}
```

## 5. Gateway 请求生命周期

认证成功后立即创建一条 `request_logs` 初始记录，记录 request_id、user_id、api_key_id、model、stream 和 `REQUEST_STARTED` 状态。后续所有可识别的错误路径必须调用统一的 `finalize_request_log`，将其更新为最终 HTTP 状态、错误码、延迟和消耗字段。

完整流程：

1. 生成/读取 request_id。
2. 解析并 hash Virtual API Key。
3. 校验 Key、用户、过期时间和有效权限。
4. 创建初始 request log。
5. 检查 RPM、TPM、并发。
6. 解析模型和路由。
7. 计算并原子预扣 Credits。
8. 调用 Provider。
9. 非流式：解析响应 usage，结算并更新日志。
10. 流式：使用 SSE buffer 按完整事件解析 usage，同时立即转发 bytes；流结束、客户端断开、上游错误和超时都执行最终化逻辑。
11. 写入 `usage_records` 和 `credit_ledger`，使用 request_id 幂等。
12. 更新日志，释放并发 slot。

并发 Guard 必须覆盖所有返回路径；SSE body 的 drop/结束路径必须释放 Redis slot。任何数据库失败都不能返回上游 Credential 或内部堆栈。

## 6. Provider 与协议边界

本次不改变 OpenAI-compatible `/v1/chat/completions` 的公开接口；Provider adapter 与公开协议保持分离。Claude 模型继续可以通过 OpenAI 格式调用，但本次不新增 Anthropic `/v1/messages`，避免文档和实现不一致。

## 7. 前端拆分

目标目录：

```text
src/
  api/client.ts
  api/auth.ts
  api/user.ts
  api/admin.ts
  stores/auth.ts
  types/api.ts
  types/credits.ts
  types/logs.ts
  layouts/UserLayout.vue
  layouts/AdminLayout.vue
  views/LoginView.vue
  views/user/UserOverviewView.vue
  views/user/UserApiKeysView.vue
  views/user/UserUsageView.vue
  views/user/UserRequestLogsView.vue
  views/user/UserCreditsView.vue
  views/user/UserDocsView.vue
  views/user/UserProfileView.vue
  views/admin/AdminOverviewView.vue
  views/admin/AdminUsersView.vue
  views/admin/AdminPlansView.vue
  views/admin/AdminProvidersView.vue
  views/admin/AdminCredentialsView.vue
  views/admin/AdminModelsView.vue
  views/admin/AdminRoutesView.vue
  views/admin/AdminLogsView.vue
  components/AppSidebar.vue
  components/AppHeader.vue
  components/PaginationBar.vue
  components/StatusPill.vue
  components/CreditTopUpDialog.vue
  components/CreditLedgerTable.vue
  App.vue
```

采用 Vue Router；`App.vue` 仅渲染 `<RouterView />`。认证 token 由 Pinia auth store 管理，API client 统一添加 Authorization 和处理 401。日志页面复用分页和状态组件，但使用量日志与请求日志保持独立 view。

管理员用户页面增加充值入口和当前余额；普通用户增加余额及 Credits 流水页面。完整 API Key 仍只在创建响应中显示一次。

## 8. 错误处理与安全

- Credits 不足返回 402 `INSUFFICIENT_CREDITS`。
- 充值 amount 为 0、超出整数范围或导致负余额时拒绝。
- 普通用户接口始终使用 JWT subject 作为 user_id，不信任客户端传入 user_id。
- 不记录完整 Virtual API Key、Provider Secret 或 prompt 内容。
- 账本充值和调整写入 audit_logs。
- 使用 request_id 关联 request_logs、usage_records 和消费账本。

## 9. 测试与验收

Rust：

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
```

Vue：

```text
npm install
npm run build
```

必须覆盖：

1. 管理员充值后余额和账本正确。
2. 普通用户只能读取自身账本和日志。
3. 成功非流式请求生成 usage、request log 和 USAGE_DEBIT。
4. 上游没有 usage 时生成 ESTIMATED usage 并消费预扣额度。
5. 余额不足不调用上游并有最终失败 request log。
6. 上游 401/403/429、超时、模型不可用均有最终 request log。
7. 并发扣费不会产生负余额，重复结算不会重复扣费。
8. App.vue 仅为入口，拆分后前端构建通过。

## 10. 明确的实现取舍

- 本次采用“管理员手动充值”作为主要余额来源，不依赖套餐自动发放 monthly_credits。
- 现有套餐限流字段继续保留，但 Credits 余额以管理员账本为准。
- 不将人民币金额存入余额；管理员按自定义比例直接输入 Credits 数量，人民币信息仅作为备注。
- 不在本次改造中实现 Anthropic 原生 Messages API。
