<script setup lang="ts">
type Row = Record<string, any>
defineProps<{ rows: Row[] }>()
const emit = defineEmits<{ review: [id: string, status: 'APPROVED' | 'REJECTED'] }>()
</script>
<template>
  <section class="panel"><div class="panel-head"><div><h2>充值订单</h2><p class="panel-subtitle">确认线下收款后，批准订单会把总 Los 分记入用户余额。</p></div></div><div class="table-wrap"><table><thead><tr><th>用户</th><th>金额</th><th>基础 Los 分</th><th>赠送</th><th>总到账</th><th>状态</th><th>时间</th><th>操作</th></tr></thead><tbody><tr v-for="row in rows" :key="row.id"><td>{{ row.email }}</td><td>¥{{ (Number(row.amount_cents || 0) / 100).toFixed(2) }}</td><td>{{ row.base_los }}</td><td>{{ row.bonus_los }}</td><td><b>{{ row.total_los }}</b></td><td>{{ row.status === 'PENDING' ? '待确认' : row.status === 'APPROVED' ? '已到账' : '已拒绝' }}</td><td>{{ new Date(row.created_at).toLocaleString('zh-CN') }}</td><td><template v-if="row.status === 'PENDING'"><button class="small" @click="emit('review', row.id, 'APPROVED')">确认到账</button><button class="small danger" @click="emit('review', row.id, 'REJECTED')">拒绝</button></template><span v-else>—</span></td></tr><tr v-if="!rows.length"><td colspan="8" class="empty">暂无充值订单</td></tr></tbody></table></div></section>
</template>
