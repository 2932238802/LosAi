<script setup lang="ts">
type Row = Record<string, any>
const props = defineProps<{ rows: Row[] }>()
const emit = defineEmits<{ review: [id: string, status: 'APPROVED' | 'REJECTED'] }>()
</script>
<template>
  <section class="panel"><div class="panel-head"><div><h2>套餐申请</h2><p class="panel-subtitle">审核通过后会自动分配套餐，Credits 仍需单独充值。</p></div></div>
    <div class="table-wrap"><table><thead><tr><th>用户</th><th>套餐</th><th>状态</th><th>申请时间</th><th>备注</th><th>操作</th></tr></thead><tbody>
      <tr v-for="item in props.rows" :key="item.id"><td>{{ item.email }}</td><td>{{ item.plan_name }}</td><td><span class="state-pill" :class="item.status === 'APPROVED' ? 'ok' : item.status === 'REJECTED' ? 'bad' : 'muted'">{{ item.status }}</span></td><td>{{ new Date(item.created_at).toLocaleString('zh-CN') }}</td><td>{{ item.note || '—' }}</td><td class="actions"><template v-if="item.status === 'PENDING'"><button class="small" @click="emit('review', item.id, 'APPROVED')">同意</button><button class="small danger" @click="emit('review', item.id, 'REJECTED')">拒绝</button></template><span v-else>—</span></td></tr>
      <tr v-if="!props.rows.length"><td colspan="6" class="empty">暂无套餐申请</td></tr>
    </tbody></table></div>
  </section>
</template>
