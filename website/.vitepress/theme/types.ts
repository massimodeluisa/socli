export interface DocsItem {
  text: string
  link: string
}

export interface DocsGroup {
  label: string
  items: DocsItem[]
}

export interface SiteTheme {
  installCommand: string
  repo: string
  license: string
  docs: DocsGroup[]
}

export type SiteKind = 'home' | 'book' | 'docs' | 'plain' | 'reader'

declare module 'vitepress' {
  namespace DefaultTheme {
    interface Config extends SiteTheme {}
  }
}
