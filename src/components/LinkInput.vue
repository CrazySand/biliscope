<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { ElButton, ElInput, type InputInstance } from 'element-plus'
import { ArrowRight, Link2, LoaderCircle, X } from '@lucide/vue'

const props = defineProps<{ busy: boolean }>()
const emit = defineEmits<{ parse: [input: string]; 'input-change': [value: string] }>()

const input = ref('')
const inputElement = ref<InputInstance>()

watch(input, value => emit('input-change', value))

async function clear() {
  input.value = ''
  await nextTick()
  inputElement.value?.focus()
}

function submit() {
  if (!props.busy && input.value.trim()) emit('parse', input.value.trim())
}
</script>

<template>
  <form class="link-form" :class="{ 'is-busy': busy }" @submit.prevent="submit">
    <ElInput
      ref="inputElement"
      v-model="input"
      class="link-input"
      aria-label="B 站视频链接或 BV 号"
      placeholder="粘贴 B 站视频链接或 BV 号"
      :disabled="busy"
      autocomplete="off"
      spellcheck="false"
    >
      <template #prefix><Link2 :size="17" class="muted" /></template>
      <template #suffix>
        <ElButton v-if="input && !busy" text class="icon-button clear-input" aria-label="清除链接" @click="clear">
          <X :size="15" />
        </ElButton>
      </template>
    </ElInput>
    <ElButton
      type="primary"
      native-type="submit"
      class="parse-button"
      :aria-label="busy ? '正在解析视频' : '解析视频'"
      :disabled="!input.trim()"
      :aria-disabled="busy || !input.trim()"
      :aria-busy="busy"
    >
      <LoaderCircle v-if="busy" :size="18" class="spin" aria-hidden="true" />
      <ArrowRight v-else :size="18" aria-hidden="true" />
    </ElButton>
  </form>
</template>
