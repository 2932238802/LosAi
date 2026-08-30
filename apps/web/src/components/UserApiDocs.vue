<script setup lang="ts">
import { computed, ref } from 'vue'

type Model = Record<string, any>
const props = defineProps<{ models: Model[]; baseUrl: string; balance?: number }>()
const emit = defineEmits<{ copy: [value: string, message?: string] }>()
const model = ref('general-chat')
const language = ref<'curl' | 'python' | 'javascript'>('curl')
const stream = ref(false)
const selectedModel = computed(() => model.value || props.models[0]?.id || props.models[0]?.model_name || 'general-chat')
const code = computed(() => {
  const name = selectedModel.value
  const url = `${props.baseUrl}/chat/completions`
  if (language.value === 'python') return `from openai import OpenAI\nclient = OpenAI(api_key="sk-gw_你的密钥", base_url="${props.baseUrl}")\nresponse = client.chat.completions.create(model="${name}", messages=[{"role":"user","content":"你好"}], stream=${stream.value ? 'True' : 'False'})`
  if (language.value === 'javascript') return `import OpenAI from "openai";\nconst client = new OpenAI({ apiKey: "sk-gw_你的密钥", baseURL: "${props.baseUrl}" });\nconst response = await client.chat.completions.create({ model: "${name}", messages: [{ role: "user", content: "你好" }], stream: ${stream.value} });`
  return `curl ${url} ${stream.value ? '-N ' : ''}\\\n  -H "Authorization: Bearer sk-gw_你的密钥" \\\n  -H "Content-Type: application/json" \\\n  -d '${JSON.stringify({ model: name, messages: [{ role: 'user', content: '你好' }], stream: stream.value })}'`
})
</script>
<template>
  <section class="panel docs docs-simple">
    <div class="docs-hero"><div><span class="eyebrow">USER API</span><h2>复制代码，开始调用</h2><p>LosToken 使用 OpenAI Chat Completions 格式，Claude 模型也通过同一格式调用。</p></div><strong class="docs-balance">余额 {{ props.balance ?? '—' }}</strong></div>
    <div class="docs-base"><div><small>API Base URL</small><code>{{ props.baseUrl }}</code></div><button class="secondary" @click="emit('copy', props.baseUrl, 'Base URL 已复制')">复制</button></div>
    <div class="docs-controls"><label>模型<select v-model="model"><option v-for="item in props.models" :key="item.id || item.model_name" :value="item.id || item.model_name">{{ item.id || item.model_name }}</option><option v-if="!props.models.length" value="general-chat">general-chat</option></select></label><label class="stream-toggle"><input v-model="stream" type="checkbox"> 流式响应</label></div>
    <div class="code-toolbar"><div class="code-tabs"><button v-for="item in (['curl', 'python', 'javascript'] as const)" :key="item" :class="{ active: language === item }" @click="language = item">{{ item === 'curl' ? 'curl' : item === 'python' ? 'Python' : 'JavaScript' }}</button></div><button class="secondary small" @click="emit('copy', code, '代码已复制')">复制代码</button></div>
    <div class="code-block"><pre>{{ code }}</pre></div>
    <div class="docs-compat"><strong>Claude 兼容说明</strong><span>Claude 模型通过上述 OpenAI 格式调用；当前暂不支持 Anthropic 原生 <code>/v1/messages</code>。</span></div>
    <details class="docs-errors"><summary>常见错误</summary><p><code>401</code> Key 无效　<code>402</code> Credits 不足　<code>404</code> 模型不可用　<code>429</code> 请求频率或并发超限</p></details>
  </section>
</template>
