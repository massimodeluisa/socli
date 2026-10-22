<script setup lang="ts">
import { ref } from 'vue'
import { useData, withBase } from 'vitepress'
import { normalizeDocPath, useCurrentPath } from '../paths'

const { theme } = useData()
const currentPath = useCurrentPath()

const browse = ref<HTMLDetailsElement | null>(null)
defineExpose({ browse })

function isCurrent(link: string): boolean {
  return normalizeDocPath(link) === currentPath.value
}
</script>

<template>
  <aside class="docs-nav">
    <details ref="browse" class="browse" open>
      <summary>Browse</summary>
      <nav aria-label="Documentation">
        <template v-for="group in theme.docs" :key="group.label">
          <p>{{ group.label }}</p>
          <a
            v-for="item in group.items"
            :key="item.link"
            :href="withBase(item.link)"
            :aria-current="isCurrent(item.link) ? 'page' : undefined"
            >{{ item.text }}</a
          >
        </template>
      </nav>
    </details>
  </aside>
</template>
