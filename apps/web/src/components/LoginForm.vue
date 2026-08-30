<script setup lang="ts">
const props = defineProps<{ busy: boolean; error: string; mode: 'login' | 'register' }>()
const emit = defineEmits<{ submit: [payload: { email: string; password: string; confirmPassword: string }]; 'update:mode': [mode: 'login' | 'register'] }>()
let email = ''
let password = ''
let confirmPassword = ''
function submit() { emit('submit', { email, password, confirmPassword }) }
</script>
<template>
  <div class="login-page">
    <form class="login-card" @submit.prevent="submit">
      <div class="eyebrow">LOS / TOKEN</div>
      <h1>{{ props.mode === 'login' ? '登录' : '注册' }}</h1>
      <label>邮箱<input v-model="email" type="email" required autocomplete="username"></label>
      <label>密码<input v-model="password" type="password" required minlength="8" :autocomplete="props.mode === 'login' ? 'current-password' : 'new-password'"></label>
      <label v-if="props.mode === 'register'">确认密码<input v-model="confirmPassword" type="password" required minlength="8"></label>
      <p v-if="props.error" class="error">{{ props.error }}</p>
      <button :disabled="props.busy">{{ props.busy ? '处理中…' : props.mode === 'login' ? '登录' : '注册' }}</button>
      <button class="text-button" type="button" @click="emit('update:mode', props.mode === 'login' ? 'register' : 'login')">{{ props.mode === 'login' ? '创建账户' : '返回登录' }}</button>
    </form>
  </div>
</template>
