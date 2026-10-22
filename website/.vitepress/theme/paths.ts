import { computed, type ComputedRef } from 'vue'
import { useData, useRoute } from 'vitepress'

export function normalizeDocPath(path: string): string {
  const trimmed = path.replace(/index\.html$/, '').replace(/\/+$/, '')
  return trimmed === '' ? '/' : trimmed
}

export function useCurrentPath(): ComputedRef<string> {
  const route = useRoute()
  const { site } = useData()
  return computed(() => {
    const base = site.value.base
    let path = route.path
    if (base && base !== '/' && path.startsWith(base)) {
      path = path.slice(base.length - 1)
    }
    return normalizeDocPath(path)
  })
}
