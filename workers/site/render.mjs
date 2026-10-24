/**
Builds the temporary socli.sh HTML from website/privacy.md, website/terms.md, and website/data-deletion.md.
The Worker serves these only when the origin is down or has no page yet.
*/

import { readFileSync, writeFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, "../..")

const css = `
:root { color-scheme: dark; }
body { margin: 0; background: #000; color: #e7e9ea; font: 18px/1.55 ui-sans-serif, system-ui, sans-serif; }
main { max-width: 40rem; margin: 0 auto; padding: 3rem 1.25rem 4rem; }
a { color: inherit; }
svg { width: 3.25rem; height: 3.25rem; display: block; margin: 0 0 1.5rem; }
h1 { font-size: 1.8rem; font-weight: 640; letter-spacing: -0.03em; margin: 0 0 1rem; }
h2 { font-size: 1.15rem; font-weight: 640; margin: 2.1rem 0 0.45rem; }
p, li { margin: 0.7rem 0; }
ul { padding-left: 1.2rem; }
code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.92em; }
nav { margin-top: 2.6rem; display: flex; flex-wrap: wrap; gap: 0.75rem 1.25rem; }
nav a { text-decoration: underline; text-underline-offset: 0.18em; }
`.trim()

export function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
}

/** Inline code and https or mailto links. Everything else is escaped. */
export function inline(src) {
  const re = /(`[^`]+`|\[[^\]]+\]\([^)\s]+\))/g
  let out = ""
  let last = 0
  for (const match of src.matchAll(re)) {
    out += escapeHtml(src.slice(last, match.index))
    const token = match[0]
    if (token.startsWith("`")) {
      out += `<code>${escapeHtml(token.slice(1, -1))}</code>`
    } else {
      const link = token.match(/^\[([^\]]+)\]\(([^)\s]+)\)$/)
      const href = link?.[2] ?? ""
      if (link && /^(https:\/\/|mailto:)/.test(href)) {
        out += `<a href="${escapeHtml(href)}">${escapeHtml(link[1])}</a>`
      } else {
        out += escapeHtml(token)
      }
    }
    last = match.index + token.length
  }
  return out + escapeHtml(src.slice(last))
}

export function renderMarkdown(markdown) {
  const lines = markdown.replace(/^---[\s\S]*?---\n/, "").replace(/\r\n/g, "\n").split("\n")
  const html = []
  let paragraph = []
  let list = []

  const flushParagraph = () => {
    if (paragraph.length === 0) return
    html.push(`<p>${inline(paragraph.join(" "))}</p>`)
    paragraph = []
  }
  const flushList = () => {
    if (list.length === 0) return
    html.push(`<ul>${list.map((item) => `<li>${inline(item)}</li>`).join("")}</ul>`)
    list = []
  }

  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed) {
      flushParagraph()
      flushList()
      continue
    }
    const heading = /^(#{1,3})\s+(.*)$/.exec(trimmed)
    if (heading) {
      flushParagraph()
      flushList()
      const level = heading[1].length
      html.push(`<h${level}>${inline(heading[2])}</h${level}>`)
      continue
    }
    if (trimmed.startsWith("- ")) {
      flushParagraph()
      list.push(trimmed.slice(2))
      continue
    }
    flushList()
    paragraph.push(trimmed)
  }
  flushParagraph()
  flushList()
  return html.join("\n")
}

export function shell({ title, canonical, description, body }) {
  const mark = readFileSync(join(root, "website/public/brand/so-dark-text.svg"), "utf8").trim()
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)}</title>
<link rel="canonical" href="${escapeHtml(canonical)}">
<meta name="description" content="${escapeHtml(description)}">
<style>${css}</style>
</head>
<body>
<main>
${mark}
${body}
<nav>
<a href="/">SoCLI</a>
<a href="/privacy.html">Privacy</a>
<a href="/terms.html">Terms</a>
<a href="/data-deletion.html">Data deletion</a>
<a href="https://github.com/massimodeluisa/socli">Source</a>
</nav>
</main>
</body>
</html>
`
}

export function pages() {
  const privacyMd = readFileSync(join(root, "website/privacy.md"), "utf8")
  const termsMd = readFileSync(join(root, "website/terms.md"), "utf8")
  const deletionMd = readFileSync(join(root, "website/data-deletion.md"), "utf8")
  const home = `<h1>SoCLI</h1>
<p>SoCLI is a free open-source client for X, Bluesky, Threads, Mastodon, and dev/null. It runs in the terminal, and the same reader can run in the browser. Massimo De Luisa maintains it from Italy.</p>
<p>Questions go to the <a href="https://github.com/massimodeluisa/socli/issues/new">GitHub issue form</a>.</p>`
  return {
    home: shell({
      title: "SoCLI",
      canonical: "https://socli.sh/",
      description:
        "SoCLI is a free open-source client for X, Bluesky, Threads, Mastodon, and dev/null.",
      body: home,
    }),
    privacy: shell({
      title: "Privacy policy · SoCLI",
      canonical: "https://socli.sh/privacy.html",
      description: "What SoCLI stores when you sign in, and the one Threads request that reaches socli.sh.",
      body: renderMarkdown(privacyMd),
    }),
    terms: shell({
      title: "Terms · SoCLI",
      canonical: "https://socli.sh/terms.html",
      description: "What you get when you use SoCLI, and what socli.sh does not store.",
      body: renderMarkdown(termsMd),
    }),
    deletion: shell({
      title: "Data deletion · SoCLI",
      canonical: "https://socli.sh/data-deletion.html",
      description: "How to remove Threads and Facebook data from SoCLI, and the address Meta can call.",
      body: renderMarkdown(deletionMd),
    }),
  }
}

export function pagesModule(built = pages()) {
  return (
    "/* Generated by workers/site/render.mjs. Do not edit by hand. */\n" +
    `export const home = ${JSON.stringify(built.home)}\n` +
    `export const privacy = ${JSON.stringify(built.privacy)}\n` +
    `export const terms = ${JSON.stringify(built.terms)}\n` +
    `export const deletion = ${JSON.stringify(built.deletion)}\n`
  )
}

const entry = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]
if (entry) {
  const target = join(here, "pages.js")
  const next = pagesModule()
  if (process.argv.includes("--check")) {
    const current = readFileSync(target, "utf8")
    if (current !== next) {
      console.error("workers/site/pages.js is stale. Run: node workers/site/render.mjs")
      process.exit(1)
    }
  } else {
    writeFileSync(target, next)
    console.log(target)
  }
}
