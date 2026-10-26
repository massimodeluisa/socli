import { defineConfig } from 'vitepress'
import llmstxt from 'vitepress-plugin-llms'
import { xRelay } from './relay'

declare const process: { env: Record<string, string | undefined> }

const origin = (process.env.SITE_ORIGIN ?? 'https://socli.sh').replace(/\/+$/, '')

const base = (() => {
  const raw = (process.env.SITE_BASE ?? '/').trim()
  const trimmed = raw.replace(/^\/+/, '').replace(/\/+$/, '')
  return trimmed ? `/${trimmed}/` : '/'
})()

const description =
  'Cross-platform TUI social media client for X, Bluesky, dev/null, Threads and Mastodon.'

const installCommand = 'npx soclish'

const readerHome = `${base}web/`
const readerBare = `${base}web`
const readerFile = `${base}web/index.html`

const themeScript = `(function(){var t="auto";try{t=localStorage.getItem("socli-theme")||"auto"}catch(e){}if(t!=="dark"&&t!=="light"&&t!=="auto")t="auto";document.documentElement.setAttribute("data-theme",t);if(location.hostname==="web.socli.sh"){document.documentElement.setAttribute("data-app","web");var path=location.pathname;if(path!==${JSON.stringify(readerHome)}&&path!==${JSON.stringify(readerBare)}&&path!==${JSON.stringify(readerFile)})location.replace(${JSON.stringify(readerHome)});}})();`

/** `/web` and `/web/` are the same reader. cleanUrls is off, so the file is web/index.html. */
function webReaderRoute() {
  return {
    name: 'socli-web-reader',
    enforce: 'pre' as const,
    configureServer(server: { middlewares: { use: (fn: ReaderRewrite) => void } }) {
      server.middlewares.use(rewriteWeb)
    },
    configurePreviewServer(server: { middlewares: { use: (fn: ReaderRewrite) => void } }) {
      server.middlewares.use(rewriteWeb)
    },
  }
}

type ReaderRewrite = (
  req: { url?: string },
  res: unknown,
  next: () => void,
) => void

const rewriteWeb: ReaderRewrite = (req, _res, next) => {
  const url = req.url ?? ''
  const query = url.indexOf('?')
  const path = query === -1 ? url : url.slice(0, query)
  const search = query === -1 ? '' : url.slice(query)
  if (path === readerBare) req.url = readerHome + search
  next()
}

const ogImageAlt =
  "SoCLI: the words Read. Reply. Post. beside the SoCLI reader in a terminal window, where the selected post shows two photos: an ocean wave and Lisbon's 25 de Abril Bridge at sunset."

export default defineConfig({
  lang: 'en',
  title: 'SoCLI',
  titleTemplate: ':title · SoCLI',
  description,
  base,
  cleanUrls: false,
  appearance: false,
  lastUpdated: false,
  srcExclude: ['parts/**'],
  outDir: '.vitepress/dist',
  cacheDir: '.vitepress/cache',
  sitemap: {
    hostname: origin + base,
  },
  head: [
    ['script', {}, themeScript],
    ['meta', { name: 'theme-color', content: '#000000' }],
    ['link', { rel: 'icon', href: base + 'favicon.ico', sizes: 'any' }],
    ['link', { rel: 'icon', href: base + 'favicon.svg', type: 'image/svg+xml' }],
    ['link', { rel: 'icon', href: base + 'favicon-32.png', type: 'image/png', sizes: '32x32' }],
    ['link', { rel: 'icon', href: base + 'favicon-16.png', type: 'image/png', sizes: '16x16' }],
    ['link', { rel: 'apple-touch-icon', href: base + 'apple-touch-icon.png' }],
    ['link', { rel: 'manifest', href: base + 'site.webmanifest' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'SoCLI' }],
    ['meta', { property: 'og:image', content: origin + base + 'og.png' }],
    ['meta', { property: 'og:image:width', content: '1200' }],
    ['meta', { property: 'og:image:height', content: '630' }],
    ['meta', { property: 'og:image:alt', content: ogImageAlt }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ['meta', { name: 'twitter:image', content: origin + base + 'og.png' }],
    ['meta', { name: 'twitter:image:alt', content: ogImageAlt }],
    [
      'script',
      { type: 'application/ld+json' },
      JSON.stringify({
        '@context': 'https://schema.org',
        '@type': 'SoftwareApplication',
        name: 'SoCLI',
        applicationCategory: 'SocialNetworkingApplication',
        operatingSystem: 'macOS, Linux',
        url: origin + base,
        description,
        image: origin + base + 'og.png',
        logo: origin + base + 'icon-512.png',
        dateModified: '2026-10-25',
        author: {
          '@type': 'Person',
          name: 'Massimo De Luisa',
          url: 'https://github.com/massimodeluisa',
        },
      }),
    ],
  ],
  transformPageData(pageData) {
    const url =
      origin +
      base +
      pageData.relativePath.replace(/(^|\/)index\.md$/, '$1').replace(/\.md$/, '.html')
    const frontmatter = pageData.frontmatter ?? (pageData.frontmatter = {})
    const head = frontmatter.head ?? (frontmatter.head = [])
    head.push(
      ['link', { rel: 'canonical', href: url }],
      ['meta', { property: 'og:url', content: url }],
      ['meta', { property: 'og:title', content: pageData.title || 'SoCLI' }],
      ['meta', { property: 'og:description', content: pageData.description || description }],
    )
  },
  markdown: {
    codeCopyButtonTitle: 'Copy',
  },
  vite: {
    plugins: [
      webReaderRoute(),
      xRelay(base),
      llmstxt({
        title: 'SoCLI',
        description,
        details:
          'SoCLI is a Rust terminal client. Agents should run it with --json or --print, never the full-screen reader. Docs: ' +
          origin +
          base +
          'docs/',
        domain: origin + base.replace(/\/$/, ''),
        excludeIndexPage: false,
        ignoreFiles: ['index.md', 'web/index.md', 'parts/**'],
      }),
    ],
    ssr: {
      noExternal: ['vitepress-plugin-llms'],
    },
  },
  themeConfig: {
    installCommand,
    repo: 'https://github.com/massimodeluisa/socli',
    license: 'https://github.com/massimodeluisa/socli/blob/main/LICENSE',
    docs: [
      {
        label: 'getting started',
        items: [
          { text: 'Quick start', link: '/docs/' },
          { text: 'Install', link: '/docs/install.html' },
          { text: 'Sign in', link: '/docs/login.html' },
        ],
      },
      {
        label: 'using SoCLI',
        items: [
          { text: 'Reader', link: '/docs/reader.html' },
          { text: 'Keys', link: '/docs/keys.html' },
          { text: 'CLI commands', link: '/docs/commands.html' },
          { text: 'Images', link: '/docs/images.html' },
          { text: 'Web implementation', link: '/docs/web.html' },
        ],
      },
      {
        label: 'reference',
        items: [
          { text: 'Networks', link: '/docs/networks.html' },
          { text: 'Configuration', link: '/docs/config.html' },
        ],
      },
      {
        label: 'for agents',
        items: [{ text: 'Agents', link: '/docs/agents.html' }],
      },
    ],
  },
})
