<script setup lang="ts">
import { computed } from 'vue'
import { useData, withBase } from 'vitepress'
import type { SiteKind } from '../types'

const props = defineProps<{ kind: SiteKind }>()

const { theme } = useData()

const keysHref = computed(() =>
  props.kind === 'home' ? '#keys' : withBase('/docs/keys.html'),
)
</script>

<template>
  <footer>
    <a :href="theme.repo">source</a>
    <a href="https://github.com/massimodeluisa/socli/issues/new">questions</a>
    <a :href="withBase('/docs/')">docs</a>
    <a :href="withBase('/privacy.html')">privacy</a>
    <a :href="withBase('/data-deletion.html')">data deletion</a>
    <a :href="withBase('/design.html')">brand</a>
    <a :href="withBase('/llms.txt')">llms.txt</a>
    <a :href="keysHref">keys</a>
    <a :href="theme.license">MIT</a>
  </footer>
</template>
