# AI API Gateway / 虚拟密钥与额度分发平台技术设计文档 v1.1（Rust Backend）

## 0. 文档定位

本文件是一份可直接交给代码 Agent 执行的 MVP 工程规格书。目标是实现一个多租户 AI API Gateway：平台发行自己的虚拟 API Key，按套餐、额度、RPM、并发等规则控制访问，再将合法请求路由到已经取得商业使用/下游服务授权的上游 AI API。

**关键约束：** 系统只接入明确允许当前业务模式的上游。本文不包含绕过上游服务条款、账号限制、风控或封禁的实现。

---

## 1. 产品目标

### 1.1 MVP 必须实现

1. 管理员创建/禁用客户账号。
2. 客户创建、查看、撤销自己的虚拟 API Key。
3. 虚拟 Key 采用 Bearer Token 方式访问 OpenAI-compatible API。
4. 支持套餐：月额度、RPM、TPM（可选）、最大并发、允许模型、到期时间。
5. 支持多个 Provider、多把上游 Credential，以及健康状态与权重。
6. 根据公开模型名完成模型映射与路由。
7. 记录每次请求的 token usage、credits、耗时、状态码、provider、model。
8. 余额/额度不足时立即拒绝请求。
9. 支持 SSE 流式返回。
10. 管理后台可查看客户、套餐、Key、额度、请求统计、Provider/上游 Key 池、审计日志。
11. Docker Compose 一键启动 Web、API、PostgreSQL、Redis。
12. 提供数据库 migration、seed、README、OpenAPI/Swagger 文档和最小测试集。

### 1.2 MVP 暂不实现

- 自动收款、支付宝/微信/Stripe。
- 发票、财税流程。
- 多区域部署。
- 复杂代理商层级。
- 自动购买第三方账号或自动规避第三方平台限制。
- Kubernetes。

---

## 2. 推荐技术栈

| 层 | 技术 | 说明 |
|---|---|---|
| Web | Vue 3 + Vite + TypeScript | Composition API，strict TypeScript |
| UI | Element Plus | 快速完成管理后台 |
| 状态 | Pinia | 登录态、用户信息、全局配置 |
| 路由 | Vue Router | user/admin 两套路由守卫 |
| HTTP | Axios | Dashboard 调 API |
| 图表 | ECharts | Usage、成本、请求量趋势 |
| Backend | Rust + Axum + Tokio | 高并发 Gateway、管理 API、SSE |
| HTTP Client | Reqwest | 上游 HTTP/SSE 调用，连接池与超时 |
| DB Access | SQLx + PostgreSQL | 编译期/类型安全 SQL、transaction、migration |
| Cache/Limit | Redis | RPM、TPM、并发、短期缓存、分布式协调 |
| Middleware | Tower / tower-http | trace、timeout、CORS、compression、request-id |
| Auth | HttpOnly Cookie session/JWT + Argon2id | Dashboard 登录 |
| API Key | Bearer virtual key + keyed hash | 对外 AI API 鉴权 |
| Observability | tracing + tracing-subscriber | JSON structured logging / spans |
| OpenAPI | utoipa（或等价方案） | 管理 API 文档与前端类型生成 |
| Deploy | Docker Compose + Nginx/Caddy | MVP 单机部署 |
| Tests | cargo test + integration tests | 单元、接口和并发测试 |

---

## 3. 总体架构

```text
                         ┌─────────────────────────┐
                         │      Vue 3 Dashboard    │
                         │ customer / admin portal │
                         └────────────┬────────────┘
                                      │ HTTPS
                                      ▼
┌──────────────┐            ┌──────────────────────┐
│ API Customer │  Bearer    │ Rust/Axum API Server│
│ SDK / Agent  ├───────────►│                      │
└──────────────┘            │ 1. Auth / API Key    │
                            │ 2. Quota / RateLimit │
                            │ 3. Billing / Usage   │
                            │ 4. Gateway Router    │
                            │ 5. Admin API         │
                            └──────┬────────┬───────┘
                                   │        │
                          ┌────────▼───┐ ┌──▼──────────┐
                          │ PostgreSQL │ │    Redis    │
                          └────────────┘ └─────────────┘
                                   │
                                   ▼
                            ┌──────────────────────┐
                            │   Provider Adapters  │
                            │ OpenAI-compatible /  │
                            │ vendor-specific APIs │
                            └───────────┬──────────┘
                                        │
                              ┌─────────▼─────────┐
                              │ Authorized Upstream│
                              │ Credential / API   │
                              └────────────────────┘
```

**设计原则：客户虚拟 Key 与上游 Credential 完全解耦。** 客户永远不获得上游真实 Key；上游切换、轮换、故障转移不影响客户配置。

### 3.1 为什么必须有后端

本项目不能做成纯 Vue 前端。以下能力必须由可信后端执行：

1. 保存和解密真实上游 Credential。
2. 校验客户虚拟 API Key。
3. 统一执行套餐、Credits、RPM、TPM、并发和到期控制。
4. 隐藏 Provider 真实地址与密钥。
5. 进行 usage 结算、幂等扣费和审计。
6. 维护 Provider health、circuit breaker 和 failover。
7. 防止客户篡改余额、绕过限制或从浏览器拿到真实 Secret。

因此 Vue 只负责 Dashboard；**真正对外提供 AI API 的组件是 Rust Gateway**。

### 3.2 统一出口（Single Egress Gateway）

所有客户请求统一进入本平台 Gateway，再由 Gateway 访问授权上游，是推荐架构：

```text
Customer A ─┐
Customer B ─┼──> api.example.com / Rust Gateway ───> Authorized Upstream
Customer C ─┘
```

它的目的应当是：Secret 隔离、配额控制、稳定出口 IP、审计、故障切换和运维。

**不得把统一出口设计成规避上游账号共享/转售检测的隐匿机制。** 不实现 IP 轮换、请求指纹伪装、设备/账号行为模拟、Header 欺骗、规避风控探测等功能。若上游要求固定出口 IP，可将 Gateway/NAT 公网 IP 提供给上游 allowlist。

---

## 4. 请求生命周期

以 `POST /v1/chat/completions` 为例：

```text
1. 客户请求 + Authorization: Bearer sk-vg_xxx
2. 解析 key prefix，查询 API Key
3. 常量时间比较 key hash
4. 检查 key 状态、用户状态、套餐状态、过期时间
5. 检查模型是否在 allowlist
6. Redis 检查 RPM / TPM / 并发
7. 检查月额度 / credits
8. 根据 public model 查 model_mapping
9. 从健康 upstream credential pool 选择目标
10. 请求上游（流式或非流式）
11. 收集 usage 与 latency
12. 计算 credits
13. 原子写入 usage_event + 更新计费聚合
14. 释放并发锁
15. 将兼容响应返回客户
```

### 4.1 失败策略

- 鉴权失败：401。
- 套餐不可用/Key 被禁用：403。
- 超 RPM/TPM/并发：429。
- 额度不足：429，错误码 `insufficient_quota`。
- 上游暂时失败：在**未向客户端输出任何响应字节之前**，最多切换一个健康 Credential 重试一次。
- SSE 已开始输出后不得透明重试，避免重复内容。
- 所有 retry 必须有全局 timeout 和 circuit breaker。

---

## 5. 角色与权限

### 5.1 CUSTOMER

- 查看自己的套餐与剩余额度。
- 创建/撤销虚拟 Key。
- 查看自己的使用统计与请求元数据。
- 使用 Playground 测试 API。
- 不可查看真实上游 Key、其他客户、平台总成本。

### 5.2 ADMIN

- 用户 CRUD / enable / disable。
- 套餐 CRUD。
- 手工分配订阅/额度。
- Provider CRUD。
- Upstream Credential 创建、轮换、启用、禁用、健康检查。
- 模型映射和 credit rate 配置。
- 查看全站 usage、成本、错误率、审计日志。

---

## 6. 核心数据模型

### 6.1 User

```ts
User {
  id: uuid
  email: string unique
  passwordHash: string
  role: 'ADMIN' | 'CUSTOMER'
  status: 'ACTIVE' | 'DISABLED'
  displayName?: string
  createdAt: datetime
  updatedAt: datetime
}
```

### 6.2 ApiKey

```ts
ApiKey {
  id: uuid
  userId: uuid
  name: string
  prefix: string           // 如 sk-vg_ab12，允许用于定位
  secretHash: string       // 只保存 hash，不保存完整明文
  status: 'ACTIVE' | 'REVOKED'
  lastUsedAt?: datetime
  expiresAt?: datetime
  createdAt: datetime
}
```

规则：完整 Key 只在创建成功时返回一次。例如：

```text
sk-vg_<publicPrefix>_<32~48 bytes random secret>
```

数据库禁止保存完整 Key。

### 6.3 Plan

```ts
Plan {
  id: uuid
  name: string
  monthlyCredits: bigint
  rpm: int
  tpm?: int
  maxConcurrency: int
  allowedModels: string[]
  enabled: boolean
}
```

### 6.4 Subscription

```ts
Subscription {
  id: uuid
  userId: uuid
  planId: uuid
  status: 'ACTIVE' | 'EXPIRED' | 'SUSPENDED'
  periodStart: datetime
  periodEnd: datetime
  creditLimit: bigint
  creditUsed: bigint
}
```

`creditLimit` 必须在开通时复制套餐额度，防止后续修改 Plan 导致历史订阅被意外改变。

### 6.5 Provider

```ts
Provider {
  id: uuid
  code: string unique
  name: string
  adapterType: 'OPENAI_COMPATIBLE' | 'CUSTOM'
  baseUrl: string
  enabled: boolean
  defaultTimeoutMs: int
}
```

### 6.6 UpstreamCredential

```ts
UpstreamCredential {
  id: uuid
  providerId: uuid
  name: string
  encryptedSecret: string
  status: 'ACTIVE' | 'DISABLED' | 'UNHEALTHY'
  weight: int
  priority: int
  maxConcurrency?: int
  lastHealthAt?: datetime
  lastErrorAt?: datetime
  errorStreak: int
  createdAt: datetime
}
```

真实上游 Secret 使用应用级 Master Key/KMS 加密，严禁明文存储或返回前端。

### 6.7 ModelMapping

```ts
ModelMapping {
  id: uuid
  publicModel: string
  providerId: uuid
  upstreamModel: string
  enabled: boolean
  inputCreditPer1K: decimal
  outputCreditPer1K: decimal
  priority: int
}
```

同一个 `publicModel` 可以映射多个 provider 作为故障转移线路。

### 6.8 UsageEvent

```ts
UsageEvent {
  id: uuid
  requestId: string unique
  userId: uuid
  apiKeyId: uuid
  subscriptionId: uuid
  providerId?: uuid
  upstreamCredentialId?: uuid
  publicModel: string
  upstreamModel?: string
  inputTokens: int
  outputTokens: int
  totalTokens: int
  creditsCharged: bigint
  latencyMs: int
  statusCode: int
  stream: boolean
  errorCode?: string
  createdAt: datetime
}
```

默认**不保存 prompt/response 正文**；只保存计费和诊断元数据。若未来允许客户开启正文日志，必须单独获得授权并配置独立 retention。

### 6.9 AuditLog

记录管理员对 User、Plan、Provider、Credential、ModelMapping、Subscription 的敏感操作。

---

## 7. Credit 计费模型

平台统一使用内部 Credits，不直接将不同上游的 token 定价暴露给客户。

```text
credits = ceil(inputTokens / 1000 * inputRate)
        + ceil(outputTokens / 1000 * outputRate)
```

要求：

1. 所有 rate 使用 Decimal，不用 float。
2. UsageEvent 是计费事实表，不允许管理员直接修改。
3. `Subscription.creditUsed` 是快速聚合值；以 UsageEvent 为最终审计依据。
4. 扣费操作必须具备幂等键 `requestId`。
5. 请求开始前进行“可用额度预检查”；请求完成后按真实 usage 结算。
6. 可设置每请求最大 token/最大 credits 风险阈值。

---

## 8. Redis 限流设计

建议 Key：

```text
rate:rpm:{apiKeyId}:{minute}
rate:tpm:{apiKeyId}:{minute}
conc:{apiKeyId}
provider:conc:{credentialId}
health:{credentialId}
```

### 8.1 RPM

使用 Redis Lua 脚本完成 `INCR + EXPIRE` 原子操作。

### 8.2 并发

进入请求时原子获取 semaphore；请求结束、异常、客户端断开时必须释放。Semaphore 设置 TTL，防止进程崩溃产生永久占位。

### 8.3 TPM

MVP 可以采用“请求前按照 max_tokens 做预留，完成后按真实 token 回补”的策略；若复杂度过高，可以先仅实现 RPM + concurrency + monthly credits，将 TPM 放到 v1.1。

---

## 9. Provider Adapter 设计

定义统一接口：

```ts
interface AiProviderAdapter {
  listModels(ctx: ProviderContext): Promise<ProviderModel[]>;
  chatCompletions(
    ctx: ProviderContext,
    request: OpenAIChatCompletionRequest
  ): Promise<OpenAIChatCompletionResponse>;
  chatCompletionsStream(
    ctx: ProviderContext,
    request: OpenAIChatCompletionRequest
  ): AsyncIterable<Uint8Array>;
  healthCheck(ctx: ProviderContext): Promise<HealthResult>;
}
```

首个实现：`OpenAICompatibleAdapter`。

禁止业务模块直接拼接供应商 URL。所有供应商差异必须封装在 Adapter 中。

---

## 10. 上游 Credential Pool 路由

### 10.1 过滤顺序

1. Provider.enabled = true
2. Credential.status = ACTIVE
3. health != unhealthy
4. 当前并发未超过 credential 限额
5. Credential 支持目标模型

### 10.2 排序建议

先 `priority ASC`，同优先级按 `weight` 做 weighted random / smooth weighted round-robin。

### 10.3 Circuit Breaker

- 连续 3 次网络/5xx 错误 → 临时 `UNHEALTHY` 60 秒。
- 后台 health job 每 30~60 秒探测。
- 探测成功后恢复 ACTIVE。
- 401/403 这类 Credential 错误直接标记为不可用并告警，不能持续重试。

---

## 11. 对外 OpenAI-Compatible API

### 11.1 GET /v1/models

只返回当前用户套餐允许且平台已启用的 public models。

### 11.2 POST /v1/chat/completions

认证：

```http
Authorization: Bearer sk-vg_xxxxxxxxx
Content-Type: application/json
```

MVP 至少支持：

```json
{
  "model": "general-chat",
  "messages": [
    {"role": "user", "content": "hello"}
  ],
  "temperature": 0.7,
  "max_tokens": 1024,
  "stream": false
}
```

流式模式使用 `text/event-stream` 并尽可能保持 OpenAI SSE chunk 兼容。

### 11.3 标准错误格式

```json
{
  "error": {
    "message": "Monthly credit quota exhausted",
    "type": "quota_error",
    "code": "insufficient_quota",
    "request_id": "req_xxx"
  }
}
```

建议错误码：

- `invalid_api_key`
- `api_key_revoked`
- `account_disabled`
- `subscription_inactive`
- `model_not_allowed`
- `rate_limit_exceeded`
- `concurrency_limit_exceeded`
- `insufficient_quota`
- `upstream_unavailable`
- `upstream_timeout`
- `internal_error`

---

## 12. Dashboard REST API

Base path: `/api`

### Auth

```text
POST   /api/auth/login
POST   /api/auth/logout
GET    /api/auth/me
```

### Customer

```text
GET    /api/dashboard/summary
GET    /api/usage?from=&to=&groupBy=day
GET    /api/usage/requests?page=&pageSize=
GET    /api/api-keys
POST   /api/api-keys
DELETE /api/api-keys/:id
GET    /api/models
POST   /api/playground/chat
```

### Admin

```text
GET/POST/PATCH /api/admin/users
GET/POST/PATCH /api/admin/plans
GET/POST/PATCH /api/admin/subscriptions
GET/POST/PATCH /api/admin/providers
GET/POST/PATCH /api/admin/upstream-credentials
POST           /api/admin/upstream-credentials/:id/test
GET/POST/PATCH /api/admin/model-mappings
GET            /api/admin/usage/summary
GET            /api/admin/health
GET            /api/admin/audit-logs
```

所有 Admin 写操作必须产生 AuditLog。

---

## 13. Vue 3 前端页面

### 13.1 Customer

```text
/login
/dashboard
/api-keys
/usage
/playground
/account
```

Dashboard 卡片：

- 当前套餐
- 本周期剩余 Credits
- 本周期已用 Credits
- 今日请求量
- 今日 token 使用量
- 最近 7/30 天 Usage 折线图
- 最近错误率

API Keys 页面：创建 Key 时弹窗仅显示一次完整 secret，并提供“复制”按钮和明确提示。

### 13.2 Admin

```text
/admin
/admin/users
/admin/plans
/admin/subscriptions
/admin/providers
/admin/upstream-credentials
/admin/model-mappings
/admin/usage
/admin/audit-logs
```

Credential 页面不得显示完整 secret；最多显示 `****abcd` 或人为设置的备注名。

---

## 14. Vue 项目目录

```text
apps/web/
  src/
    api/
      auth.ts
      apiKeys.ts
      usage.ts
      admin.ts
      client.ts
    components/
      common/
      charts/
      api-key/
    layouts/
      CustomerLayout.vue
      AdminLayout.vue
    router/
      index.ts
      guards.ts
    stores/
      auth.ts
      app.ts
    types/
      api.ts
      domain.ts
    utils/
      format.ts
      error.ts
    views/
      auth/LoginView.vue
      customer/DashboardView.vue
      customer/ApiKeysView.vue
      customer/UsageView.vue
      customer/PlaygroundView.vue
      admin/AdminDashboardView.vue
      admin/UsersView.vue
      admin/PlansView.vue
      admin/SubscriptionsView.vue
      admin/ProvidersView.vue
      admin/CredentialsView.vue
      admin/ModelMappingsView.vue
      admin/AuditLogsView.vue
    App.vue
    main.ts
```

要求：页面不可直接写 Axios；统一通过 `src/api` 调用。权限逻辑放 router guards 和后端双重校验，不能只靠前端隐藏按钮。

---

## 15. Rust Backend 项目目录

```text
apps/api/
  Cargo.toml
  src/
    main.rs
    app.rs
    config.rs
    state.rs
    error.rs
    routes/
      auth.rs
      dashboard.rs
      admin.rs
      openai.rs
      health.rs
    middleware/
      request_id.rs
      api_key_auth.rs
      dashboard_auth.rs
      rate_limit.rs
      concurrency.rs
      audit.rs
    domain/
      user.rs
      api_key.rs
      plan.rs
      subscription.rs
      provider.rs
      usage.rs
    services/
      auth.rs
      api_keys.rs
      billing.rs
      quota.rs
      usage.rs
      provider_router.rs
      credential_vault.rs
    providers/
      mod.rs
      adapter.rs
      openai_compatible.rs
      types.rs
    repositories/
      users.rs
      api_keys.rs
      subscriptions.rs
      providers.rs
      usage.rs
    redis/
      limiter.rs
      semaphore.rs
    jobs/
      health_probe.rs
    observability/
      tracing.rs
  migrations/
  tests/
    api_key_auth.rs
    quota.rs
    rate_limit.rs
    billing_idempotency.rs
    provider_failover.rs
    rbac.rs
```

Rust 侧要求：

- `AppState` 只保存可安全 clone 的连接池/客户端/配置句柄，例如 `PgPool`、Redis `ConnectionManager`、`reqwest::Client`、`Arc<Config>`。
- handler 保持薄层，只负责解析请求、调用 service、映射 response；计费、路由和配额逻辑不得散落在 route 中。
- PostgreSQL 使用 SQLx transaction；生产 migration 以 SQL migration 文件为准。
- Redis 使用异步连接管理，Lua 脚本实现需要原子性的 RPM/semaphore 操作。
- 上游 HTTP 必须复用一个或少量 `reqwest::Client`，禁止每个请求新建 Client。
- 对外 SSE 使用 Axum SSE/streaming body；客户端断开时要取消上游读取并释放 semaphore。

Provider adapter：

```rust
#[async_trait::async_trait]
pub trait AiProviderAdapter: Send + Sync {
    async fn list_models(&self, ctx: &ProviderContext) -> Result<Vec<ProviderModel>, GatewayError>;

    async fn chat_completions(
        &self,
        ctx: &ProviderContext,
        request: OpenAiChatCompletionRequest,
    ) -> Result<OpenAiChatCompletionResponse, GatewayError>;

    async fn chat_completions_stream(
        &self,
        ctx: &ProviderContext,
        request: OpenAiChatCompletionRequest,
    ) -> Result<ProviderByteStream, GatewayError>;

    async fn health_check(&self, ctx: &ProviderContext) -> Result<HealthResult, GatewayError>;
}
```

Adapter 返回统一内部类型，Gateway 层再转换为 OpenAI-compatible JSON/SSE。

---

## 16. Monorepo 结构

```text
ai-api-gateway/
  apps/
    web/                  # Vue 3 / TypeScript
    api/                  # Rust / Axum
      Cargo.toml
      src/
      migrations/
      tests/
  packages/
    api-contract/         # OpenAPI schema / generated TS client
  docker/
  docker-compose.yml
  pnpm-workspace.yaml     # 仅管理前端 workspace
  Cargo.toml              # Rust workspace，可选
  .env.example
  README.md
```

前后端不再共享 TypeScript runtime DTO。**以 OpenAPI schema 作为契约源**：Rust 后端生成/维护 OpenAPI，Vue 前端通过生成器生成 TypeScript API 类型与 client，避免 Rust struct 与 TS interface 长期漂移。

---

## 17. 安全要求

### 17.1 客户 API Key

- 使用 Rust CSPRNG（`rand_core::OsRng` / 等价安全随机源）生成，至少 256-bit 随机性。
- 数据库只保存 prefix + keyed hash。推荐对高熵 API secret 使用 HMAC-SHA-256（服务端 pepper）或等价方案；不要为了“慢哈希”而把每次 API 请求都做昂贵 Argon2。
- 只在创建时显示一次完整 secret。
- 比较摘要时使用 constant-time comparison。
- 支持撤销与过期时间。

### 17.2 上游 Credential

- 使用 AES-256-GCM 或 KMS/Secrets Manager 加密。
- Master Key 只存在运行环境，不写数据库。
- 管理 API 不允许读取回完整 Secret。
- 任何日志禁止打印 Authorization header。

### 17.3 Dashboard

- 密码 Argon2id。
- JWT 放 HttpOnly + Secure + SameSite Cookie。
- 生产环境 HTTPS only。
- Admin endpoint 强制 RBAC。
- Login 设置 IP + account rate limit。
- CORS 只允许 Dashboard origin。

### 17.4 请求日志

默认只记录元数据，不记录 prompt / completion 正文，不记录完整 API Key，不记录上游 Authorization。

### 17.5 出口与上游访问策略

- 生产环境优先使用固定公网出口 IP，便于上游 allowlist、审计和故障排查。
- 不使用住宅代理、动态代理池或 IP 轮换来隐藏业务来源。
- 统一设置明确的 `User-Agent`，例如 `YourProductGateway/1.0`；不得伪装官方客户端。
- 转发时删除客户提供的 `Authorization`、`Host` 和危险 hop-by-hop headers，再由 Adapter 生成上游请求。
- 对每个 Provider 配置连接池、connect timeout、request timeout、最大响应体和最大 SSE 空闲时间。
- 上游返回 401/403/账号限制类错误时，停止该 Credential 的自动重试并触发管理员告警。
- Rate limit 只用于容量、公平使用、成本控制与防滥用；不用于模拟“正常个人使用行为”。

---

## 18. 一致性与并发

### 18.1 Credits

不能仅依赖前端显示的余额。网关请求前与结算时均在后端校验。

MVP 推荐：

- Redis 保存快速 quota cache。
- PostgreSQL 保存最终事实。
- 扣费使用数据库 transaction。
- UsageEvent 以 requestId unique 保证幂等。
- 每次扣费同时 update `Subscription.creditUsed`。

如果并发较高，v1.1 再改为 ledger + reservation 模型。

### 18.2 请求 ID

每次请求生成：

```text
req_<uuid/ulid>
```

写入响应 header：

```http
x-request-id: req_xxx
```

日志、UsageEvent、错误响应都关联这个 request id。

---

## 19. Health 与监控

### Endpoint

```text
GET /health/live
GET /health/ready
```

`ready` 至少检查 DB 与 Redis。

管理员 Dashboard 指标：

- requests/min
- success rate
- p50/p95 latency
- 429 rate
- upstream 5xx rate
- provider availability
- credential health
- credits sold/used（MVP 可先只统计 used）

日志采用 JSON structured logging，至少字段：

```text
requestId, userId, apiKeyId, model, providerId,
credentialId, statusCode, latencyMs, errorCode
```

---

## 20. Docker Compose

服务：

```text
web
api
postgres
redis
```

`.env.example` 至少：

```env
APP_ENV=development
RUST_LOG=info,ai_api_gateway=debug
DATABASE_URL=postgresql://...
REDIS_URL=redis://redis:6379
AUTH_SECRET=replace_me
API_KEY_PEPPER=replace_me
CREDENTIAL_MASTER_KEY=replace_me
WEB_ORIGIN=http://localhost:5173
API_PUBLIC_BASE_URL=http://localhost:3000
```

生产环境 secret 不得提交 Git。

### 20.1 推荐部署拓扑

```text
Internet
   │
Caddy/Nginx (TLS, body limit)
   │
Rust Gateway (private network)
   ├── PostgreSQL (private only)
   ├── Redis (private only)
   └── Fixed NAT/Public Egress ──> Authorized Providers
```

数据库与 Redis 不暴露公网；只有反向代理暴露 443。Rust Gateway 可以单机起步，后续通过无状态实例横向扩容；计费状态依赖 PostgreSQL/Redis，不依赖本机内存。

---

## 21. MVP 初始化数据

Seed：

1. 一个 ADMIN 用户（密码从环境变量读取，首次启动后要求修改）。
2. 一个 `Starter` Plan。
3. 一个示例 public model：`general-chat`。
4. 不写入任何真实上游 Secret。

---

## 22. API 兼容策略

MVP 优先兼容 OpenAI Chat Completions；不要一开始覆盖所有 OpenAI API。

顺序：

```text
v1.0  /v1/models + /v1/chat/completions
v1.1  /v1/responses
v1.2  embeddings（若业务需要）
```

对不支持的字段：

- 明确返回 400；
- 不允许静默忽略会改变语义的重要字段。

---

## 22.1 上游授权与运营边界

Provider 增加以下运营字段（不参与规避风控，仅用于确保接入资格）：

```text
commercialUseApproved: boolean
downstreamResaleApproved: boolean
authorizationReference?: string   # 合同/工单/邮件编号，不存敏感正文
authorizationExpiresAt?: datetime
```

生产环境只有 `commercialUseApproved=true` 且业务需要时 `downstreamResaleApproved=true` 的 Provider 才允许启用。授权到期前触发管理员告警。

系统不实现任何“隐藏客户数量”“伪造官方客户端”“模拟单用户行为”的策略。运营侧需要控制风险时，使用明确套餐、固定 RPM/TPM、并发上限、月 Credits 和每请求上限。

---

## 23. 核心业务规则

1. 一个 User 可拥有多把虚拟 API Key，但共享同一 Subscription 额度。
2. 删除/撤销 Key 不影响 Usage 历史。
3. 套餐过期立即停止 API 访问。
4. Admin 禁用用户后，其所有 API Key 立即失效。
5. 公共模型名与实际 Provider 模型解耦。
6. Provider/credential 故障不向客户暴露真实供应商秘钥。
7. 客户不可自行指定任意 `base_url` 或 provider。
8. 所有上游接入必须由管理员配置。
9. 真实上游 Secret 不进入前端 bundle、浏览器 localStorage、业务日志。
10. 上游授权/合规状态属于 Provider 的运营前置条件，而不是通过代码绕过的限制。

---

## 24. Agent 实现阶段

### Phase 1 - Scaffold

- 建立 Vue 前端 workspace + Rust Cargo workspace。
- 创建 Vue 3/Vite/TS Web。
- 创建 Rust/Axum/Tokio API。
- PostgreSQL + Redis Docker Compose。
- SQLx migrations + repository 层。
- 前端 ESLint/Prettier/strict TS；后端 rustfmt/clippy。

### Phase 2 - Auth + Customer

- User、login/logout/me。
- ApiKey 生成/撤销。
- Plan/Subscription。
- Customer Dashboard 骨架。

### Phase 3 - Gateway

- Bearer API Key Guard。
- `/v1/models`。
- `/v1/chat/completions` 非流式。
- OpenAICompatibleAdapter。
- ModelMapping。
- Credential encryption。

### Phase 4 - Quota + Streaming

- RPM/concurrency Redis limiter。
- Credit 计算和幂等 UsageEvent。
- SSE streaming。
- request id / error schema。

### Phase 5 - Admin

- Users/Plans/Subscriptions。
- Providers/Credentials/ModelMappings。
- Credential test。
- Audit log。
- Usage charts。

### Phase 6 - Hardening

- Circuit breaker。
- Health jobs。
- Integration tests。
- Docker production build。
- README + Swagger。

---

## 25. 最低验收标准

Agent 生成代码后，必须满足：

- [ ] `docker compose up` 可以启动完整环境。
- [ ] 管理员可登录并创建客户。
- [ ] 可给客户分配套餐。
- [ ] 客户可创建虚拟 Key，完整 Key 只出现一次。
- [ ] 数据库中不存在虚拟 Key 明文。
- [ ] 管理员可配置 Provider、加密 Credential 和模型映射。
- [ ] 客户可通过虚拟 Key 调 `/v1/chat/completions`。
- [ ] `stream=false` 与 `stream=true` 均可工作。
- [ ] 超 RPM 返回 429。
- [ ] 超并发返回 429。
- [ ] Credits 用尽返回 `insufficient_quota`。
- [ ] 每个成功/失败请求均有 requestId 和 UsageEvent/诊断记录（鉴权前失败可只记安全日志）。
- [ ] Credential 失败时可以切换健康线路，且不会向客户泄露 Secret。
- [ ] 客户只能看到自己的数据。
- [ ] ADMIN 写操作有 AuditLog。
- [ ] README 包含本地启动、migration、seed、创建首个 provider 的说明。
- [ ] Swagger 可以查看 Dashboard 管理 API 文档。
- [ ] 核心路径具有自动化测试。
- [ ] Rust 后端通过 `cargo fmt --check`。
- [ ] Rust 后端通过 `cargo clippy -- -D warnings`。
- [ ] Gateway 在客户端主动断开 SSE 后能释放 Redis 并发占位和上游连接。
- [ ] 上游 HTTP client 被复用，未出现每请求创建新 client 的实现。

---

## 26. 直接交给代码 Agent 的执行提示词

你是本项目的主程 Agent。请严格按照《AI API Gateway / 虚拟密钥与额度分发平台技术设计文档 v1.1（Rust Backend）》实现一个可运行 MVP，不要只生成 UI Demo。

技术栈固定为 Vue 3 + Vite + TypeScript + Pinia + Vue Router + Element Plus；后端为 Rust + Axum + Tokio + SQLx + PostgreSQL + Redis + Reqwest；使用 Docker Compose。前端使用 pnpm，后端使用 Cargo。

执行规则：

1. 先建立完整目录、SQL migrations、Docker Compose、`.env.example` 和 README，再实现业务。
2. 前端 TypeScript 开启 strict；Rust 必须通过 `cargo fmt --check`、`cargo clippy -- -D warnings`。
3. 前端不得直连上游 AI Provider；所有请求经过后端 Gateway。
4. 虚拟 API Key 只保存 hash；上游 Credential 加密保存；严禁日志打印任何 Secret。
5. 实现 `/v1/models` 和 `/v1/chat/completions`，同时支持普通 JSON 和 SSE stream。
6. 实现 User、ApiKey、Plan、Subscription、Provider、UpstreamCredential、ModelMapping、UsageEvent、AuditLog。
7. 实现 RPM、最大并发、monthly credits。
8. 实现 admin/customer 权限隔离。
9. 实现 Provider Adapter，不允许把某家供应商逻辑散落在 Gateway service 中。
10. 所有数据库变化使用 SQLx migration，不使用运行时自动改表作为生产方案。
11. 提供 seed，但不得包含任何真实 API Secret。
12. 提供测试：API key auth、quota、rate limit、billing idempotency、provider failover、RBAC。
13. 代码中出现未实现核心逻辑的 TODO 视为未完成；非核心增强项可以 TODO，但必须在 README 的 Roadmap 标记。
14. 完成每个 Phase 后运行前端 lint/typecheck/tests，以及后端 fmt/clippy/test，并修复错误后再进入下一 Phase。
15. 最终输出：完整源码、启动说明、默认页面截图非必需、Swagger 地址、示例 curl。

示例最终调用：

```bash
curl http://localhost:3000/v1/chat/completions \\
  -H 'Authorization: Bearer sk-vg_xxx' \\
  -H 'Content-Type: application/json' \\
  -d '{
    "model": "general-chat",
    "messages": [{"role":"user","content":"hello"}],
    "stream": false
  }'
```

**验收以“能启动、能登录、能创建 Key、能受配额控制地完成一次真实授权上游调用”为准，不以静态页面数量为准。**

---

## 27. 后续版本建议

### v1.2

- Quota reservation / ledger。
- TPM。
- `/v1/responses`。
- 客户自定义 webhook 告警。
- 低余额提醒。
- Provider 成本核算与毛利 dashboard。

### v1.3

- 自动充值/支付。
- 优惠码。
- 代理商层级。
- 每用户独立 model pricing。
- 多实例部署与 Redis distributed lock 优化。

### v2

- 多区域 Gateway。
- 高可用 PostgreSQL/Redis。
- Provider 动态成本路由。
- SLA、告警中心、运营后台。
