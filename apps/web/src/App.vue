<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import LoginForm from './components/LoginForm.vue'
import UserApiDocs from './components/UserApiDocs.vue'
import UserPlans from './components/UserPlans.vue'
import AdminPlanRequests from './components/AdminPlanRequests.vue'

type Row = Record<string, any>
type Role = 'ADMIN' | 'CUSTOMER' | ''
type Toast = { type: 'success' | 'error' | 'info'; message: string }
type AdminView = 'overview' | 'users' | 'plans' | 'plan-requests' | 'keys' | 'providers' | 'credentials' | 'models' | 'routes' | 'usage' | 'logs' | 'audit'
type UserView = 'overview' | 'keys' | 'plans' | 'usage' | 'logs' | 'docs' | 'profile'

const token = ref(localStorage.getItem('lostoken_token') || '')
const role = ref<Role>((localStorage.getItem('lostoken_role') as Role) || '')
const email = ref('')
const password = ref('')
const authMode = ref<'login' | 'register'>('login')
const confirmPassword = ref('')
const busy = ref(false)
const rowBusy = ref('')
const error = ref('')
const toast = ref<Toast | null>(null)
const modal = ref('')
const editing = ref<Row | null>(null)
const secretOnce = ref('')
const adminView = ref<AdminView>('overview')
const userView = ref<UserView>('overview')
const dashboard = ref<Row>({})
const rows = ref<Row[]>([])
const rowsLoading = ref(false)
const rowsError = ref('')
const page = ref(1)
const pageSize = ref(20)
const total = ref(0)
const totalPages = ref(0)

const plans = ref<Row[]>([])
const users = ref<Row[]>([])
const keys = ref<Row[]>([])
const providers = ref<Row[]>([])
const credentials = ref<Row[]>([])
const models = ref<Row[]>([])
const routes = ref<Row[]>([])
const profile = ref<Row>({})
const subscription = ref<Row>({})
const billing = ref<Row>({})
const userPlans = ref<Row[]>([])
const planRequests = ref<Row[]>([])
const form = ref<Row>({})
let toastTimer: ReturnType<typeof setTimeout> | undefined

const loggedIn = computed(() => Boolean(token.value))
const isAdmin = computed(() => role.value === 'ADMIN')
const apiBaseUrl = (import.meta.env.VITE_API_BASE_URL || (import.meta.env.DEV ? '' : 'https://api.losai.site')).replace(/\/+$/, '')
const publicBaseUrl = computed(() => `${apiBaseUrl || window.location.origin}/v1`)
const docsTab = ref<'quickstart' | 'openai' | 'claude' | 'limits'>('quickstart')
const docsCodeTab = ref<'curl' | 'python' | 'javascript'>('curl')
const docsStream = ref(false)
const docsModel = ref('general-chat')
const docsModels = computed(() => list(models.value.length ? models.value : dashboard.value.models))
const selectedDocsModel = computed(() => docsModel.value || docsModels.value[0]?.id || 'general-chat')
const adminTitles: Record<AdminView, string> = { overview: '平台总览', users: '用户管理', plans: '套餐管理', 'plan-requests': '套餐申请', keys: '平台 API 密钥', providers: 'Provider 管理', credentials: 'Provider 凭证', models: '模型管理', routes: '模型路由', usage: '平台使用量', logs: '请求日志', audit: '审计日志' }
const userTitles: Record<UserView, string> = { overview: '账户总览', keys: '我的 API 密钥', plans: '套餐与余额', usage: '使用量日志', logs: '请求日志', docs: 'API 文档', profile: '账户资料' }

async function request(path: string, init: RequestInit = {}) {
  const headers = new Headers(init.headers)
  if (init.body) headers.set('Content-Type', 'application/json')
  if (token.value) headers.set('Authorization', `Bearer ${token.value}`)
  const response = await fetch(`${apiBaseUrl}${path}`, { ...init, headers })
  const body = response.status === 204 ? {} : await response.json().catch(() => ({}))
  if (!response.ok) {
    if (response.status === 401) logout()
    throw new Error(body?.error?.message || body?.message || `请求失败（${response.status}）`)
  }
  return body
}
const list = (body: any): Row[] => Array.isArray(body) ? body : Array.isArray(body?.data) ? body.data : []
function notify(message: string, type: Toast['type'] = 'success') { toast.value = { message, type }; if (toastTimer) clearTimeout(toastTimer); toastTimer = setTimeout(() => toast.value = null, 4000) }
function fail(cause: unknown, fallback = '操作失败') { notify(cause instanceof Error ? cause.message : fallback, 'error') }
function fmt(value: any) { if (value === null || value === undefined || value === '') return '—'; if (typeof value === 'boolean') return value ? '启用' : '禁用'; if (typeof value === 'string' && value.includes('T')) return new Date(value).toLocaleString('zh-CN'); return String(value) }
function copyText(value: string, message = '已复制到剪贴板') {
  navigator.clipboard?.writeText(value).then(() => notify(message)).catch(() => notify('复制失败，请手动复制', 'error'))
}
function docsCode(tab = docsCodeTab.value) {
  const model = selectedDocsModel.value
  const base = publicBaseUrl.value
  if (tab === 'python') return `from openai import OpenAI\n\nclient = OpenAI(\n    api_key="sk-gw_你的密钥",\n    base_url="${base}"\n)\n\nresponse = client.chat.completions.create(\n    model="${model}",\n    messages=[{"role": "user", "content": "你好"}],\n    stream=${docsStream.value ? 'True' : 'False'}\n)\n\nif ${docsStream.value ? 'True' : 'False'}:\n    for chunk in response:\n        print(chunk.choices[0].delta.content or "", end="")\nelse:\n    print(response.choices[0].message.content)`
  if (tab === 'javascript') return `import OpenAI from "openai";\n\nconst client = new OpenAI({\n  apiKey: "sk-gw_你的密钥",\n  baseURL: "${base}"\n});\n\nconst response = await client.chat.completions.create({\n  model: "${model}",\n  messages: [{ role: "user", content: "你好" }],\n  stream: ${docsStream.value}\n});\n\nif (${docsStream.value}) {\n  for await (const chunk of response) {\n    process.stdout.write(chunk.choices[0]?.delta?.content || "");\n  }\n} else {\n  console.log(response.choices[0].message.content);\n}`
  return `curl ${base}/chat/completions \\\n  -N \\\n  -H "Authorization: Bearer sk-gw_你的密钥" \\\n  -H "Content-Type: application/json" \\\n  -d '${JSON.stringify({ model, messages: [{ role: 'user', content: '你好' }], stream: docsStream.value })}'`
}

function logout() {
  token.value = ''
  role.value = ''
  localStorage.removeItem('lostoken_token')
  localStorage.removeItem('lostoken_role')
  rows.value = []
  rowsError.value = ''
  total.value = 0
  totalPages.value = 0
}

async function authenticate() { busy.value = true; error.value = ''; try { const path = authMode.value === 'login' ? '/auth/login' : '/auth/register'; const payload = authMode.value === 'login' ? { email: email.value, password: password.value } : { email: email.value, password: password.value, confirm_password: confirmPassword.value }; const result = await request(path, { method: 'POST', body: JSON.stringify(payload) }); token.value = result.access_token; role.value = result.role || 'CUSTOMER'; localStorage.setItem('lostoken_token', token.value); localStorage.setItem('lostoken_role', role.value); if (isAdmin.value) await loadAdminAll(); else await loadUserAll() } catch (e) { error.value = e instanceof Error ? e.message : '认证失败' } finally { busy.value = false } }

async function loadAdminAll() {
  const results = await Promise.allSettled(['/admin/dashboard', '/admin/users', '/admin/plans', '/admin/api-keys', '/admin/providers', '/admin/credentials', '/admin/models', '/admin/routes', '/admin/plan-requests'].map(path => request(path)))
  const value = (i: number) => results[i].status === 'fulfilled' ? (results[i] as PromiseFulfilledResult<any>).value : {}
  dashboard.value = value(0); users.value = list(value(1)); plans.value = list(value(2)); keys.value = list(value(3)); providers.value = list(value(4)); credentials.value = list(value(5)); models.value = list(value(6)); routes.value = list(value(7)); planRequests.value = list(value(8)); await loadAdminView()
}
async function loadAdminView() { if (!isAdmin.value) return; const map: Partial<Record<AdminView, string>> = { users: '/admin/users', plans: '/admin/plans', 'plan-requests': '/admin/plan-requests', keys: '/admin/api-keys', providers: '/admin/providers', credentials: '/admin/credentials', models: '/admin/models', routes: '/admin/routes', usage: '/admin/usage', logs: '/admin/request-logs', audit: '/admin/audit-logs' }; const path = map[adminView.value]; if (path) rows.value = list(await request(path)); else rows.value = [] }
async function selectAdmin(view: AdminView) { adminView.value = view; error.value = ''; try { await loadAdminView() } catch (e) { fail(e, '加载数据失败') } }

async function loadUserAll() { const results = await Promise.allSettled(['/user/dashboard', '/user/profile', '/user/subscription', '/user/api-keys', '/v1/models', '/user/plans', '/user/plan-requests', '/user/billing'].map(path => request(path))); const value = (i: number) => results[i].status === 'fulfilled' ? (results[i] as PromiseFulfilledResult<any>).value : {}; dashboard.value = value(0); profile.value = value(1); subscription.value = value(2); keys.value = list(value(3)); models.value = list(value(4)); userPlans.value = list(value(5)); planRequests.value = list(value(6)); billing.value = value(7); await loadUserView() }
async function loadUserView() {
  const map: Partial<Record<UserView, string>> = { keys: '/user/api-keys', usage: '/user/usage', logs: '/user/request-logs' }
  const path = map[userView.value]
  if (!path) { rows.value = []; total.value = 0; totalPages.value = 0; return }
  rowsLoading.value = true
  rowsError.value = ''
  try {
    const result = await request(`${path}?page=${page.value}&page_size=${pageSize.value}`)
    rows.value = list(result)
    total.value = Number(result.total || 0)
    totalPages.value = Number(result.total_pages || 0)
  } catch (e) {
    rows.value = []
    total.value = 0
    totalPages.value = 0
    rowsError.value = e instanceof Error ? e.message : '加载日志失败'
    throw e
  } finally { rowsLoading.value = false }
}
async function selectUser(view: UserView) {
  userView.value = view
  page.value = 1
  total.value = 0
  totalPages.value = 0
  rows.value = []
  rowsError.value = ''
  error.value = ''
  try {
    await loadUserView()
  } catch (e) {
    fail(e, '加载数据失败')
  }
}

async function applyPlan(planId: string) {
  try {
    await request('/user/plan-requests', { method: 'POST', body: JSON.stringify({ plan_id: planId }) })
    notify('套餐申请已提交')
    await loadUserAll()
  } catch (e) {
    fail(e, '申请套餐失败')
  }
}
async function reviewPlanRequest(id: string, status: 'APPROVED' | 'REJECTED') { const note = prompt(status === 'APPROVED' ? '审核备注（可选）' : '拒绝原因（可选）') ?? ''; try { await request(`/admin/plan-requests/${id}`, { method: 'PATCH', body: JSON.stringify({ status, note }) }); notify('审核完成'); await loadAdminAll() } catch (e) { fail(e, '审核失败') } }
async function changeUserPage(next: number) {
  if (next < 1 || (totalPages.value > 0 && next > totalPages.value) || rowsLoading.value) return
  page.value = next
  try { await loadUserView() } catch (e) { fail(e, '加载日志失败') }
}
function copyRequestId(value: string) { copyText(value, '请求 ID 已复制') }


function openCreate(kind: string) { editing.value = null; modal.value = kind; form.value = defaults(kind) }
function openEdit(kind: string, item: Row) { editing.value = item; modal.value = kind; const copy = { ...item, password: '', secret: '' }; if (copy.expires_at) copy.expires_at = new Date(copy.expires_at).toISOString().slice(0, 16); form.value = copy }
function closeModal() { modal.value = ''; editing.value = null; form.value = {} }
function defaults(kind: string): Row { const firstProvider = providers.value[0]?.id || ''; const firstModel = models.value[0]?.id || ''; return kind === 'user' ? { email: '', password: '', role: 'CUSTOMER', plan_id: null, credits_balance: 0 } : kind === 'plan' ? { name: '', monthly_credits: 0, rpm_limit: 60, tpm_limit: 0, max_concurrency: 2, monthly_request_limit: 0 } : kind === 'key' ? { user_id: isAdmin.value ? (users.value[0]?.id || '') : undefined, name: '默认密钥', expires_at: null } : kind === 'provider' ? { name: 'AICodeWith', adapter: 'openai_compatible', base_url: 'https://api.aicodewith.ai/v1', priority: 100, weight: 100 } : kind === 'credential' ? { provider_id: firstProvider, label: '主凭证', secret: '', priority: 100, weight: 100 } : kind === 'model' ? { model_name: '', provider_id: firstProvider, upstream_model: '', input_rate_micros: 1000, output_rate_micros: 2000, priority: 100, weight: 100 } : kind === 'route' ? { model_id: firstModel, provider_id: firstProvider, upstream_model: '', priority: 100, weight: 100 } : {} }

async function save() { busy.value = true; try { const kind = modal.value; const id = editing.value?.id; let path = ''; let method = id ? 'PATCH' : 'POST'; if (kind === 'user') path = id ? `/admin/users/${id}` : '/admin/users'; if (kind === 'plan') path = id ? `/admin/plans/${id}` : '/admin/plans'; if (kind === 'key') path = isAdmin.value ? (id ? `/admin/api-keys/${id}` : '/admin/api-keys') : (id ? `/user/api-keys/${id}` : '/user/api-keys'); if (kind === 'provider') path = id ? `/admin/providers/${id}` : '/admin/providers'; if (kind === 'credential') path = id ? `/admin/credentials/${id}` : '/admin/credentials'; if (kind === 'model') path = id ? `/admin/models/${id}` : '/admin/models'; if (kind === 'route') path = id ? `/admin/routes/${id}` : '/admin/routes'; const payload = { ...form.value }; if (payload.expires_at) payload.expires_at = new Date(payload.expires_at).toISOString(); else if (kind === 'key') payload.expires_at = null; if (id && kind === 'user') delete payload.password; if (id && kind === 'credential' && !payload.secret) delete payload.secret; if (id && kind === 'model') { delete payload.provider_id; delete payload.upstream_model; delete payload.priority; delete payload.weight } const result = await request(path, { method, body: JSON.stringify(payload) }); if (result.secret) { secretOnce.value = result.secret } closeModal(); notify(result.message || '保存成功'); if (isAdmin.value) await loadAdminAll(); else await loadUserAll() } catch (e) { fail(e, '保存失败') } finally { busy.value = false } }

async function setStatus(kind: string, item: Row, enabled: boolean) { rowBusy.value = item.id; try { let path = ''; let payload: any = { enabled }; if (kind === 'user') path = `/admin/users/${item.id}/status`; if (kind === 'plan') path = `/admin/plans/${item.id}/status`; if (kind === 'key') path = isAdmin.value ? `/admin/api-keys/${item.id}/status` : `/user/api-keys/${item.id}/status`; if (kind === 'provider') path = `/admin/providers/${item.id}/status`; if (kind === 'credential') { path = `/admin/credentials/${item.id}/status`; payload = { status: enabled ? 'ACTIVE' : 'DISABLED' } } if (kind === 'model') path = `/admin/models/${item.id}/status`; if (kind === 'route') path = `/admin/routes/${item.id}/status`; const result = await request(path, { method: 'PATCH', body: JSON.stringify(payload) }); item.enabled = enabled; if (kind === 'credential') item.status = payload.status; notify(result.message || '状态已更新'); if (isAdmin.value) await loadAdminAll(); else await loadUserAll() } catch (e) { fail(e, '状态更新失败') } finally { rowBusy.value = '' } }
async function remove(kind: string, item: Row) {
  const displayName = item.name || item.model_name || item.label || item.key_prefix || item.id
  if (!confirm(`删除“${displayName}”？此操作不可撤销。`)) return
  rowBusy.value = item.id
  try {
    const root = kind === 'user' ? 'users' : kind === 'plan' ? 'plans' : kind === 'key' ? 'api-keys' : kind === 'provider' ? 'providers' : kind === 'credential' ? 'credentials' : kind === 'model' ? 'models' : 'routes'
    const prefix = isAdmin.value ? '/admin' : '/user'
    const result = await request(`${prefix}/${root}/${item.id}`, { method: 'DELETE' })
    if (kind === 'model' && result.deleted !== true) throw new Error('服务端未确认删除，请重试')
    rows.value = rows.value.filter(row => row.id !== item.id)
    if (kind === 'model') {
      models.value = models.value.filter(row => row.id !== item.id)
      routes.value = routes.value.filter(row => row.model_id !== item.id)
    }
    notify(result.message || '已删除')
    if (isAdmin.value) await loadAdminAll(); else await loadUserAll()
    if (kind === 'model' && models.value.some(row => row.id === item.id)) throw new Error('数据校准失败，模型仍然存在')
  } catch (e) {
    fail(e, '删除失败')
    if (isAdmin.value) await loadAdminAll().catch(() => undefined); else await loadUserAll().catch(() => undefined)
  } finally { rowBusy.value = '' }
}
async function check(kind: string, item: Row) { rowBusy.value = item.id; notify('正在检测，请稍候…', 'info'); try { const path = kind === 'model' ? '/admin/models/check' : `/admin/${kind}s/${item.id}/check`; const init = kind === 'model' ? { method: 'POST', body: JSON.stringify({ model_id: item.id }) } : { method: 'POST' }; const result = await request(path, init); notify(`${result.message}${result.latency_ms !== undefined ? ` · ${result.latency_ms} ms` : ''}`, result.ok === false ? 'error' : 'success'); await loadAdminAll() } catch (e) { fail(e, '检测失败') } finally { rowBusy.value = '' } }
async function resetPassword(item: Row) { const value = prompt(`为 ${item.email} 设置新密码（至少 8 位）`); if (!value) return; try { const result = await request(`/admin/users/${item.id}/password`, { method: 'PATCH', body: JSON.stringify({ password: value }) }); notify(result.message) } catch (e) { fail(e, '密码重置失败') } }

function fieldsFor(kind: string) { return kind === 'user' ? [['email', '邮箱', 'email'], ...(!editing.value ? [['password', '初始密码', 'password']] : []), ['role', '角色', 'select-role'], ['plan_id', '套餐', 'select-plan'], ['credits_balance', 'Credits 余额', 'number']] : kind === 'plan' ? [['name', '套餐名称', 'text'], ['monthly_credits', '套餐 Credits', 'number'], ['rpm_limit', 'RPM', 'number'], ['tpm_limit', 'TPM', 'number'], ['max_concurrency', '最大并发', 'number'], ['monthly_request_limit', '每月请求次数（0=不限）', 'number']] : kind === 'key' ? [...(isAdmin.value && !editing.value ? [['user_id', '所属用户', 'select-user']] : []), ['name', '密钥名称', 'text'], ['expires_at', '过期时间', 'datetime-local']] : kind === 'provider' ? [['name', '名称', 'text'], ['adapter', '适配器', 'text'], ['base_url', 'Base URL', 'url'], ['priority', '优先级', 'number'], ['weight', '权重', 'number']] : kind === 'credential' ? [...(!editing.value ? [['provider_id', 'Provider', 'select-provider']] : []), ['label', '凭证名称', 'text'], ['secret', '上游 API Key（留空表示不替换）', 'password'], ['priority', '优先级', 'number'], ['weight', '权重', 'number']] : kind === 'model' ? [...(!editing.value ? [['provider_id', 'Provider', 'select-provider']] : []), ['model_name', '客户端模型', 'text'], ...(!editing.value ? [['upstream_model', '上游模型', 'text']] : []), ['input_rate_micros', '输入倍率', 'number'], ['output_rate_micros', '输出倍率', 'number'], ...(!editing.value ? [['priority', '优先级', 'number'], ['weight', '权重', 'number']] : [])] : [['model_id', '客户端模型', 'select-model'], ['provider_id', 'Provider', 'select-provider'], ['upstream_model', '上游模型', 'text'], ['priority', '优先级', 'number'], ['weight', '权重', 'number']] }
function labelOf(kind: string) { return { user: '用户', plan: '套餐', key: 'API 密钥', provider: 'Provider', credential: '凭证', model: '模型', route: '模型路由' }[kind] || kind }
const currentKind = computed(() => adminView.value === 'users' ? 'user' : adminView.value === 'plans' ? 'plan' : adminView.value === 'keys' ? 'key' : adminView.value === 'providers' ? 'provider' : adminView.value === 'credentials' ? 'credential' : adminView.value === 'models' ? 'model' : adminView.value === 'routes' ? 'route' : '')
const columns = computed(() => adminView.value === 'users' ? [['email', '邮箱'], ['role', '角色'], ['credits_balance', '余额'], ['enabled', '状态']] : adminView.value === 'plans' ? [['name', '名称'], ['monthly_credits', 'Credits'], ['rpm_limit', 'RPM'], ['tpm_limit', 'TPM'], ['max_concurrency', '并发'], ['monthly_request_limit', '月请求数'], ['enabled', '状态']] : adminView.value === 'keys' ? [['name', '名称'], ['key_prefix', '前缀'], ['enabled', '状态'], ['expires_at', '过期时间']] : adminView.value === 'providers' ? [['name', '名称'], ['base_url', 'Base URL'], ['priority', '优先级'], ['weight', '权重'], ['enabled', '状态']] : adminView.value === 'credentials' ? [['provider_name', 'Provider'], ['label', '名称'], ['secret_fingerprint', '指纹'], ['status', '状态'], ['last_used_at', '最近使用']] : adminView.value === 'models' ? [['model_name', '模型'], ['input_rate_micros', '输入倍率'], ['output_rate_micros', '输出倍率'], ['enabled', '状态']] : adminView.value === 'routes' ? [['model_name', '模型'], ['provider_name', 'Provider'], ['upstream_model', '上游模型'], ['priority', '优先级'], ['weight', '权重'], ['enabled', '状态']] : adminView.value === 'usage' ? [['created_at', '时间'], ['user_id', '用户'], ['model', '模型'], ['input_tokens', '输入 Token'], ['output_tokens', '输出 Token'], ['credits', 'Credits'], ['status', '状态']] : adminView.value === 'logs' ? [['created_at', '时间'], ['request_id', '请求 ID'], ['model', '模型'], ['status_code', '状态码'], ['latency_ms', '延迟(ms)'], ['error_code', '错误']] : [['created_at', '时间'], ['actor_email', '操作者'], ['action', '动作'], ['resource_type', '资源'], ['resource_id', '资源 ID']])

onMounted(async () => { if (!loggedIn.value) return; try { if (isAdmin.value) await loadAdminAll(); else await loadUserAll() } catch (e) { fail(e, '恢复会话失败') } })
</script>

<template>
  <LoginForm
    v-if="!loggedIn"
    :busy="busy"
    :error="error"
    :mode="authMode"
    @submit="({ email: nextEmail, password: nextPassword, confirmPassword: nextConfirm }) => { email = nextEmail; password = nextPassword; confirmPassword = nextConfirm; authenticate() }"
    @update:mode="authMode = $event"
  />

  <div v-else class="shell">
    <aside>
      <div class="brand">LOS / TOKEN<small>{{ isAdmin ? '管理员控制台' : '用户控制台' }}</small></div>
      <nav v-if="isAdmin"><button
          v-for="item in ([['overview', '平台总览'], ['users', '用户管理'], ['plans', '套餐管理'], ['plan-requests', '套餐申请'], ['keys', '平台 API 密钥'], ['providers', 'Provider 管理'], ['credentials', 'Provider 凭证'], ['models', '模型管理'], ['routes', '模型路由'], ['usage', '平台使用量'], ['logs', '请求日志'], ['audit', '审计日志']] as const)"
          :key="item[0]" :class="{ active: adminView === item[0] }" @click="selectAdmin(item[0])">{{ item[1] }}</button></nav>
      <nav v-else><button
          v-for="item in ([['overview', '账户总览'], ['keys', '我的 API 密钥'], ['plans', '套餐与余额'], ['usage', '使用量日志'], ['logs', '请求日志'], ['docs', 'API 文档'], ['profile', '账户资料']] as const)"
          :key="item[0]" :class="{ active: userView === item[0] }" @click="selectUser(item[0])">{{ item[1] }}</button></nav>
      <button class="logout" @click="logout">退出登录</button>
    </aside>
    <main>
      <header>
        <div><span class="eyebrow">{{ isAdmin ? '控制面 / ADMIN' : '工作台 / USER' }}</span>
          <h1>{{ isAdmin ? adminTitles[adminView] : userTitles[userView] }}</h1>
        </div><button class="refresh" @click="isAdmin ? loadAdminAll() : loadUserAll()">刷新数据</button>
      </header>
      <div v-if="toast" class="toast" :class="`toast-${toast.type}`">{{ toast.message }}</div>

      <template v-if="isAdmin">
        <section v-if="adminView === 'overview'" class="grid metrics">
          <article><small>累计请求</small><strong>{{ dashboard.requests || 0 }}</strong></article>
          <article><small>累计 Token</small><strong>{{ dashboard.tokens || 0 }}</strong></article>
          <article><small>累计 Credits</small><strong>{{ dashboard.credits || 0 }}</strong></article>
          <article><small>活跃用户</small><strong>{{ dashboard.users || 0 }}</strong></article>
        </section>
        <AdminPlanRequests v-else-if="adminView === 'plan-requests'" :rows="planRequests" @review="reviewPlanRequest" />
        <section v-else class="panel">
          <div class="panel-head">
            <h2>{{ adminTitles[adminView] }}</h2><button v-if="currentKind" @click="openCreate(currentKind)">新增{{ labelOf(currentKind) }}</button>
          </div>
          <div class="table-wrap">
            <table>
              <thead>
                <tr>
                  <th v-for="column in columns" :key="column[0]">{{ column[1] }}</th>
                  <th v-if="currentKind">操作</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="item in rows" :key="item.id">
                  <td v-for="column in columns" :key="column[0]">{{ fmt(item[column[0]]) }}</td>
                  <td v-if="currentKind" class="actions"><button v-if="!['key'].includes(currentKind)" class="small"
                      @click="openEdit(currentKind, item)">编辑</button><button
                      v-if="['provider', 'credential', 'model', 'route'].includes(currentKind)" class="small"
                      :disabled="rowBusy === item.id" @click="check(currentKind, item)">检测</button><button class="small"
                      :disabled="rowBusy === item.id"
                      @click="setStatus(currentKind, item, currentKind === 'credential' ? item.status !== 'ACTIVE' : !item.enabled)">{{ (currentKind === 'credential' ? item.status === 'ACTIVE' : item.enabled) ? '禁用' : '启用' }}</button><button
                      v-if="currentKind === 'user'" class="small" @click="resetPassword(item)">重置密码</button><button
                      class="small danger" :disabled="rowBusy === item.id"
                      @click="remove(currentKind, item)">{{ rowBusy === item.id ? '处理中…' : '删除' }}</button></td>
                </tr>
                <tr v-if="rows.length === 0">
                  <td :colspan="columns.length + (currentKind ? 1 : 0)" class="empty">暂无数据。{{ currentKind ? '点击右上角创建第一条记录。' : '' }}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>
      </template>

      <template v-else>
        <section v-if="userView === 'overview'" class="grid metrics">
          <article><small>账户余额</small><strong>{{ dashboard.balance || 0 }}</strong></article>
          <article><small>累计请求</small><strong>{{ dashboard.totalRequests || 0 }}</strong></article>
          <article><small>输入 Token</small><strong>{{ dashboard.inputTokens || 0 }}</strong></article>
          <article><small>输出 Token</small><strong>{{ dashboard.outputTokens || 0 }}</strong></article>
        </section>
        <UserApiDocs
          v-else-if="userView === 'docs'"
          :models="docsModels"
          :base-url="publicBaseUrl"
          :balance="dashboard.balance"
          @copy="copyText"
        />
        <UserPlans v-else-if="userView === 'plans'" :plans="userPlans" :requests="planRequests" :billing="billing" @apply="applyPlan" @refresh="loadUserAll" />
        <section v-else-if="userView === 'profile'" class="panel">
          <h2>账户资料</h2>
          <dl>
            <dd>{{ profile.email }}</dd>
            <dt>角色</dt>
            <dd>普通用户</dd>
            <dt>账户状态</dt>
            <dd>{{ profile.enabled ? '已启用' : '已禁用' }}</dd>
            <dt>当前套餐</dt>
            <dd>{{ subscription.name || '未订阅' }}</dd>
            <dt>RPM / TPM / 并发</dt>
            <dd>{{ subscription.rpm_limit || 0 }} / {{ subscription.tpm_limit || 0 }} / {{ subscription.max_concurrency || 0 }}</dd>
          </dl>
        </section>
        <section v-else class="panel">
          <div class="panel-head">
            <div>
              <h2>{{ userTitles[userView] }}</h2>
              <p v-if="userView === 'usage' || userView === 'logs'" class="panel-subtitle">共 {{ total }} 条记录 · 第 {{ page }} /
                {{ totalPages || 1 }} 页</p>
            </div><button v-if="userView === 'keys'" @click="openCreate('key')">创建密钥</button>
          </div>
          <div v-if="rowsError" class="load-error">{{ rowsError }} <button class="small" @click="loadUserView">重试</button>
          </div>
          <div class="table-wrap">
            <table>
              <thead>
                <tr><template v-if="userView === 'keys'">
                    <th>名称</th>
                    <th>前缀</th>
                    <th>状态</th>
                    <th>创建时间</th>
                    <th>操作</th>
                  </template><template v-else-if="userView === 'usage'">
                    <th>时间</th>
                    <th>请求 ID</th>
                    <th>模型</th>
                    <th>输入</th>
                    <th>输出</th>
                    <th>总 Token</th>
                    <th>Credits</th>
                    <th>流式</th>
                    <th>状态</th>
                  </template><template v-else>
                    <th>时间</th>
                    <th>请求 ID</th>
                    <th>模型</th>
                    <th>状态码</th>
                    <th>延迟</th>
                    <th>流式</th>
                    <th>错误</th>
                  </template></tr>
              </thead>
              <tbody>
                <tr v-if="rowsLoading">
                  <td :colspan="userView === 'keys' ? 5 : userView === 'usage' ? 9 : 7" class="empty">加载中…</td>
                </tr><template v-else>
                  <tr v-for="item in rows" :key="item.id || item.request_id"><template v-if="userView === 'keys'">
                      <td>{{ item.name }}</td>
                      <td>{{ item.key_prefix }}••••</td>
                      <td><span class="state-pill" :class="item.enabled ? 'ok' : 'muted'">{{ item.enabled ? '启用' : '禁用' }}</span>
                      </td>
                      <td>{{ fmt(item.created_at) }}</td>
                      <td class="actions"><button class="small" @click="openEdit('key', item)">编辑</button><button
                          class="small"
                          @click="setStatus('key', item, !item.enabled)">{{ item.enabled ? '禁用' : '启用' }}</button><button
                          class="small danger" @click="remove('key', item)">删除</button></td>
                    </template><template v-else-if="userView === 'usage'">
                      <td>{{ fmt(item.created_at) }}</td>
                      <td><button class="link-button"
                          @click="copyRequestId(item.request_id)">{{ String(item.request_id).slice(0, 8) }}…</button></td>
                      <td>{{ item.model || '—' }}</td>
                      <td>{{ item.input_tokens }}</td>
                      <td>{{ item.output_tokens }}</td>
                      <td>{{ (item.total_tokens ?? Number(item.input_tokens || 0) + Number(item.output_tokens || 0)) }}</td>
                      <td>{{ item.credits }}</td>
                      <td>{{ item.stream ? '是' : '否' }}</td>
                      <td><span class="state-pill ok">{{ item.status }}</span></td>
                    </template><template v-else>
                      <td>{{ fmt(item.created_at) }}</td>
                      <td><button class="link-button"
                          @click="copyRequestId(item.request_id)">{{ String(item.request_id).slice(0, 8) }}…</button></td>
                      <td>{{ item.model || '—' }}</td>
                      <td><span class="state-pill" :class="item.status_code >= 400 ? 'bad' : 'ok'">{{ item.status_code }}</span>
                      </td>
                      <td>{{ item.latency_ms }} ms</td>
                      <td>{{ item.stream ? '是' : '否' }}</td>
                      <td>{{ item.error_code || '—' }}</td>
                    </template>
                  </tr>
                  <tr v-if="rows.length === 0">
                    <td :colspan="userView === 'keys' ? 5 : userView === 'usage' ? 9 : 7" class="empty">暂无数据</td>
                  </tr>
                </template>
              </tbody>
            </table>
          </div>
          <div v-if="userView === 'usage' || userView === 'logs'" class="pagination"><button class="secondary small"
              :disabled="page <= 1 || rowsLoading" @click="changeUserPage(page - 1)">上一页</button><span>第 {{ page }} /
              {{ totalPages || 1 }}
              页</span><button class="secondary small" :disabled="totalPages === 0 || page >= totalPages || rowsLoading"
              @click="changeUserPage(page + 1)">下一页</button></div>
        </section>
      </template>
    </main>
  </div>

  <div v-if="modal" class="modal-backdrop" @click.self="closeModal">
    <form class="modal-card" @submit.prevent="save">
      <div class="modal-head">
        <div><span class="eyebrow">资源配置</span>
          <h2>{{ editing ? '编辑' : '新增' }}{{ labelOf(modal) }}</h2>
        </div><button type="button" class="icon-button" @click="closeModal">×</button>
      </div>
      <div class="form-grid"><label v-for="field in fieldsFor(modal)" :key="field[0]"
          :class="{ wide: field[0] === 'secret' }">{{ field[1] }}<select v-if="field[2] === 'select-role'"
            v-model="form[field[0]]">
            <option value="CUSTOMER">普通用户</option>
            <option value="ADMIN">管理员</option>
          </select><select v-else-if="field[2] === 'select-plan'" v-model="form[field[0]]">
            <option :value="null">未分配</option>
            <option v-for="item in plans" :value="item.id" :key="item.id">{{ item.name }}</option>
          </select><select v-else-if="field[2] === 'select-user'" v-model="form[field[0]]">
            <option v-for="item in users" :value="item.id" :key="item.id">{{ item.email }}</option>
          </select><select v-else-if="field[2] === 'select-provider'" v-model="form[field[0]]">
            <option v-for="item in providers" :value="item.id" :key="item.id">{{ item.name }}</option>
          </select><select v-else-if="field[2] === 'select-model'" v-model="form[field[0]]">
            <option v-for="item in models" :value="item.id" :key="item.id">{{ item.model_name }}</option>
          </select><input v-else v-model="form[field[0]]" :type="field[2]"
            :required="!editing || !['password', 'secret'].includes(field[0])"></label></div>
      <div class="modal-actions"><button type="button" class="secondary" @click="closeModal">取消</button><button
          :disabled="busy">{{ busy ? '保存中…' : '保存更改' }}</button></div>
    </form>
  </div>
  <div v-if="secretOnce" class="modal-backdrop">
    <div class="modal-card"><span class="eyebrow">仅显示一次</span>
      <h2>API 密钥创建成功</h2>
      <p>请立即复制并安全保存。关闭后无法再次查看完整密钥。</p><code class="secret">{{ secretOnce }}</code>
      <div class="modal-actions"><button type="button"
          @click="navigator.clipboard.writeText(secretOnce); notify('密钥已复制')">复制密钥</button><button type="button"
          class="secondary" @click="secretOnce = ''">我已安全保存</button></div>
    </div>
  </div>
</template>
