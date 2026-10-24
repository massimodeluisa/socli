import { resolve, sep } from 'node:path'

const root = resolve(process.argv[2] ?? '')
if (!process.argv[2]) throw new Error('usage: bun scripts/serve-soclib.ts PACKAGED_LIBRARY_DIRECTORY')
const server = Bun.serve({
  hostname: '127.0.0.1',
  port: Number(process.env.SOCLIB_TEST_PORT ?? 5178),
  async fetch(request) {
    const url = new URL(request.url)
    if (url.pathname === '/') return new Response(Bun.file('scripts/test-soclib.html'), { headers: { 'content-type': 'text/html' } })
    const path = resolve(root, `.${decodeURIComponent(url.pathname)}`)
    if (!path.startsWith(root + sep)) return new Response('not found', { status: 404 })
    const file = Bun.file(path)
    return await file.exists() ? new Response(file) : new Response('not found', { status: 404 })
  },
})
console.log(`soclib browser checks: ${server.url}`)
