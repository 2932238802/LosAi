<script setup lang="ts">
import { ref } from 'vue'
type Row = Record<string, any>
const props = defineProps<{ plans: Row[]; requests: Row[]; billing: Row }>()
const emit = defineEmits<{ apply: [planId: string]; refresh: [] }>()
const applying = ref('')
function apply(planId: string) {
  applying.value = planId
  emit('apply', planId)
  setTimeout(() => { applying.value = '' }, 1000)
}
function limit(value: any) { return Number(value) > 0 ? value : '不限' }
function statusText(value: string) { return ({ PENDING: '等待审核', APPROVED: '已开通', REJECTED: '已拒绝', CANCELLED: '已取消' } as Record<string, string>)[value] || value }
</script>
<template>
  <div class="user-plans-page">
    <section class="grid metrics">
      <article><small>Credits 余额</small><strong>{{ props.billing.balance ?? 0 }}</strong></article>
      <article><small>当前套餐</small><strong>{{ props.billing.plan_name || '未订阅' }}</strong></article>
      <article><small>本月请求</small><strong>{{ props.billing.monthly_requests ?? 0 }} / {{ limit(props.billing.monthly_request_limit) }}</strong></article>
      <article><small>RPM / 并发</small><strong>{{ limit(props.billing.rpm_limit) }} / {{ limit(props.billing.max_concurrency) }}</strong></article>
    </section>
    <section class="panel">
      <div class="panel-head"><div><h2>选择套餐</h2><p class="panel-subtitle">选择后提交申请，管理员审核通过后才会生效。</p></div><button class="secondary small" @click="emit('refresh')">刷新</button></div>
      <div class="plan-cards">
        <article v-for="plan in props.plans" :key="plan.id" class="plan-card">
          <h3>{{ plan.name }}</h3>
          <p class="plan-credit">{{ plan.monthly_credits }} Credits</p>
          <dl><div><dt>RPM</dt><dd>{{ limit(plan.rpm_limit) }}</dd></div><div><dt>TPM</dt><dd>{{ limit(plan.tpm_limit) }}</dd></div><div><dt>最大并发</dt><dd>{{ limit(plan.max_concurrency) }}</dd></div><div><dt>月请求数</dt><dd>{{ limit(plan.monthly_request_limit) }}</dd></div></dl>
          <button :disabled="applying === plan.id" @click="apply(plan.id)">{{ applying === plan.id ? '提交中…' : '申请此套餐' }}</button>
        </article>
        <p v-if="!props.plans.length" class="empty">暂无可申请的套餐</p>
      </div>
    </section>
    <section class="panel">
      <div class="panel-head"><h2>我的申请</h2></div>
      <div class="table-wrap"><table><thead><tr><th>套餐</th><th>状态</th><th>备注</th><th>申请时间</th></tr></thead><tbody><tr v-for="item in props.requests" :key="item.id"><td>{{ item.plan_name }}</td><td><span class="state-pill" :class="item.status === 'APPROVED' ? 'ok' : item.status === 'REJECTED' ? 'bad' : 'muted'">{{ statusText(item.status) }}</span></td><td>{{ item.note || '—' }}</td><td>{{ new Date(item.created_at).toLocaleString('zh-CN') }}</td></tr><tr v-if="!props.requests.length"><td colspan="4" class="empty">暂无申请记录</td></tr></tbody></table></div>
    </section>
  </div>
</template>
