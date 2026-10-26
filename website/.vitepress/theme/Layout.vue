<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Content, useData, useRoute, withBase } from 'vitepress'
import SiteHeader from './components/SiteHeader.vue'
import SiteFooter from './components/SiteFooter.vue'
import DocsNav from './components/DocsNav.vue'
import CopyPage from './components/CopyPage.vue'
import { normalizeDocPath, useCurrentPath } from './paths'
import type { SiteKind } from './types'

const { page, frontmatter, theme } = useData()
const route = useRoute()
const currentPath = useCurrentPath()

const kind = computed<SiteKind>(() => {
  if (frontmatter.value.layout === 'home') return 'home'
  if (frontmatter.value.layout === 'book') return 'book'
  if (frontmatter.value.layout === 'reader') return 'reader'
  if ((page.value.relativePath ?? '').startsWith('docs/')) return 'docs'
  return 'plain'
})

const flatDocs = computed(() => theme.value.docs.flatMap((group) => group.items))

const next = computed(() => {
  const items = flatDocs.value
  if (!items.length) return { text: 'SoCLI', link: '/' }
  const index = items.findIndex((item) => normalizeDocPath(item.link) === currentPath.value)
  return items[(index + 1) % items.length] ?? items[0]
})

const away = ref(false)
const siteHeader = ref<{ header: HTMLElement | null } | null>(null)
const docsNav = ref<{ browse: HTMLDetailsElement | null } | null>(null)

let lastY = 0
let queued = false
let frame = 0
let wide: MediaQueryList | null = null
let reduce: MediaQueryList | null = null

function measure(): void {
  const header = siteHeader.value?.header
  if (!header) return
  const root = document.documentElement.style
  root.setProperty('--header-h', header.offsetHeight + 'px')
  if (kind.value === 'docs') {
    const summary = docsNav.value?.browse?.querySelector('summary')
    root.setProperty('--chrome-h', header.offsetHeight + (summary?.offsetHeight ?? 0) + 'px')
  }
}

function foldNav(): void {
  const browse = docsNav.value?.browse
  if (browse && wide) browse.open = wide.matches
  measure()
}

function onScroll(): void {
  if (queued) return
  queued = true
  frame = requestAnimationFrame(() => {
    queued = false
    const y = window.scrollY
    if (reduce?.matches || (kind.value !== 'docs' && !wide?.matches)) {
      away.value = false
      lastY = y
      return
    }
    const dy = y - lastY
    if (y <= 4) {
      away.value = false
    } else if (dy > 6) {
      away.value = true
      const browse = docsNav.value?.browse
      if (kind.value === 'docs' && wide && !wide.matches && browse) browse.open = false
    } else if (dy < -6) {
      away.value = false
    }
    lastY = y
  })
}

function onWideChange(): void {
  foldNav()
}

function onReduceChange(): void {
  if (reduce?.matches) away.value = false
}

onMounted(() => {
  wide = window.matchMedia('(min-width: 801px)')
  reduce = window.matchMedia('(prefers-reduced-motion: reduce)')
  lastY = window.scrollY
  foldNav()
  wide.addEventListener('change', onWideChange)
  reduce.addEventListener('change', onReduceChange)
  window.addEventListener('resize', measure)
  window.addEventListener('scroll', onScroll, { passive: true })
})

onBeforeUnmount(() => {
  wide?.removeEventListener('change', onWideChange)
  reduce?.removeEventListener('change', onReduceChange)
  window.removeEventListener('resize', measure)
  window.removeEventListener('scroll', onScroll)
  if (frame) cancelAnimationFrame(frame)
})

watch(
  () => route.path,
  () => {
    lastY = window.scrollY
    foldNav()
  },
  { flush: 'post' },
)
</script>

<template>
  <div v-if="kind === 'reader'" class="site reader">
    <main id="content">
      <Content />
    </main>
  </div>
  <div v-else :class="['site', kind, { 'chrome-away': away }]">
    <a class="skip" href="#content">Skip to content</a>
    <SiteHeader ref="siteHeader" :kind="kind" />
    <main v-if="page.isNotFound" id="content" class="doc">
      <h1>Page not found</h1>
      <p>
        That page is not on this site. <a :href="withBase('/')">SoCLI home</a> or
        <a :href="withBase('/docs/')">the docs</a>.
      </p>
    </main>
    <div v-else-if="kind === 'docs'" class="docs-shell">
      <DocsNav ref="docsNav" />
      <div class="docs-body">
        <div class="docbar">
          <CopyPage :key="route.path" />
        </div>
        <main class="doc" id="content">
          <Content />
          <p class="next">Next: <a :href="withBase(next.link)">{{ next.text }}</a></p>
        </main>
      </div>
    </div>
    <main v-else id="content">
      <Content />
    </main>
    <SiteFooter :kind="kind" />
  </div>
</template>
