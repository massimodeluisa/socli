<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import { useData, withBase } from 'vitepress'
import { write } from '../copy'

const { page } = useData()

const label = ref('Copy for LLM')
let timer: ReturnType<typeof setTimeout> | null = null

function markdownPath(relativePath: string): string {
  return relativePath.replace(/\/index\.md$/, '.md')
}

async function copyPage(): Promise<void> {
  try {
    const response = await fetch(withBase('/' + markdownPath(page.value.relativePath)))
    if (!response.ok) throw new Error(response.statusText)
    await write(await response.text())
    label.value = 'copied'
  } catch {
    label.value = 'copy failed'
  }
  if (timer) clearTimeout(timer)
  timer = setTimeout(() => {
    label.value = 'Copy for LLM'
  }, 1600)
}

onBeforeUnmount(() => {
  if (timer) clearTimeout(timer)
})
</script>

<template>
  <button type="button" class="copy-llm" @click="copyPage">{{ label }}</button>
</template>
