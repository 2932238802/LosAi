<script setup lang="ts">
type Row = Record<string, any>
const props = defineProps<{ offers: Row[]; orders: Row[]; balance: number }>()
const emit = defineEmits<{ purchase: [payload: { offer_id?: string; amount_cents?: number; note?: string }] }>()
function money(cents: number) { return `¥${(Number(cents || 0) / 100).toFixed(2)}` }
function purchase(offer?: Row) {
  if (offer) return emit('purchase', { offer_id: offer.id })
  const value = prompt('请输入充值金额（元，最低 10 元）')
  if (!value) return
  const amount = Number(value)
  if (!Number.isFinite(amount) || amount < 10) return alert('充值金额不能低于 10 元')
  emit('purchase', { amount_cents: Math.round(amount * 100), note: '自定义充值' })
}
</script>
<template>
  <section class="billing-page">
    <div class="balance-card"><small>Los 分余额</small><strong>{{ props.balance || 0 }}</strong><p>1 元 = 50 Los 分 · 充值申请经管理员确认后到账</p></div>
    <div class="panel"><div class="panel-head"><div><h2>充值 Los 分</h2><p class="panel-subtitle">选择充值档位，线下付款后等待管理员确认。</p></div><button @click="purchase()">自定义充值</button></div>
      <div class="offer-grid"><article v-for="offer in props.offers" :key="offer.id" class="offer-card"><h3>{{ offer.name }}</h3><strong>{{ money(offer.amount_cents) }}</strong><p>{{ offer.description }}</p><div>到账 <b>{{ offer.total_los }}</b> Los 分<span v-if="offer.bonus_los">（赠送 {{ offer.bonus_los }}）</span></div><button @click="purchase(offer)">提交申请</button></article></div>
    </div>
    <div class="panel"><div class="panel-head"><h2>我的充值申请</h2></div><div class="table-wrap"><table><thead><tr><th>时间</th><th>金额</th><th>到账 Los 分</th><th>状态</th><th>备注</th></tr></thead><tbody><tr v-for="order in props.orders" :key="order.id"><td>{{ new Date(order.created_at).toLocaleString('zh-CN') }}</td><td>{{ money(order.amount_cents) }}</td><td>{{ order.total_los }}</td><td>{{ order.status === 'APPROVED' ? '已到账' : order.status === 'REJECTED' ? '已拒绝' : '待确认' }}</td><td>{{ order.note || '—' }}</td></tr><tr v-if="!props.orders.length"><td colspan="5" class="empty">暂无充值申请</td></tr></tbody></table></div></div>
  </section>
</template>
