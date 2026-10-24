import { describe, expect, test } from 'bun:test'
import { ReaderClient, canonicalPageAddress, isAuthorize } from '../website/.vitepress/theme/wasm-reader'

class FakeWorker extends EventTarget {
  messages: any[] = []
  terminated = false
  constructor(private readonly respond = true) { super() }
  postMessage(message: object) {
    this.messages.push(message)
    if (this.respond) queueMicrotask(() => this.reply())
  }
  reply(text = 'reader frame') {
    this.dispatchEvent(new MessageEvent('message', { data: { type: 'frame', text, frame: new ArrayBuffer(0), pictures: [] } }))
  }
  terminate() { this.terminated = true }
}

describe('reader worker lifecycle', () => {
  test('bounds queued input instead of retaining unlimited requests', async () => {
    const worker = new FakeWorker(false)
    const client = new ReaderClient({ worker: worker as unknown as Worker })
    const results = Promise.allSettled(Array.from({ length: 257 }, () => client.tick()))
    client.stop()
    const last = (await results).at(-1)
    expect(last.status).toBe('rejected')
    if (last.status === 'rejected') expect(last.reason.message).toContain('queue is full')
  })
  test('queues requests and rejects both active and queued requests when disposed', async () => {
    const worker = new FakeWorker(false)
    const client = new ReaderClient({ worker: worker as unknown as Worker })
    const first = client.tick()
    const second = client.tick()
    const completed = Promise.allSettled([first, second])
    await Promise.resolve()
    expect(worker.messages).toHaveLength(1)
    client.stop()
    const results = await completed
    expect(results.every((result) => result.status === 'rejected')).toBe(true)
    expect(worker.terminated).toBe(true)
    await expect(client.tick()).rejects.toThrow('stopped')
  })

  test('worker failure rejects future requests without hanging', async () => {
    const worker = new FakeWorker(false)
    const client = new ReaderClient({ worker: worker as unknown as Worker })
    const response = client.tick()
    const completion = Promise.allSettled([response])
    await Promise.resolve()
    worker.dispatchEvent(new Event('messageerror'))
    expect((await completion)[0].status).toBe('rejected')
    expect(worker.terminated).toBe(true)
    await expect(client.tick()).rejects.toThrow('decoded')
  })

  test('host configuration, resource URLs and save errors stay with the supplied adapters', async () => {
    const worker = new FakeWorker()
    const saves: string[] = []
    const errors: Error[] = []
    const client = new ReaderClient({
      worker: worker as unknown as Worker,
      config: '[ui]\nicons="unicode"',
      page: 'https://example.test/reader',
      oauth: null,
      wasmUrl: 'https://example.test/assets/engine.wasm',
      relayUrl: null,
      onConfig: async (text) => { saves.push(text); throw new Error('storage unavailable') },
      onError: (error) => errors.push(error),
    })
    const boot = client.boot(80, 24, true, 0.5)
    await Promise.resolve()
    worker.dispatchEvent(new MessageEvent('message', { data: { type: 'config', text: 'updated config' } }))
    expect((await boot).text).toBe('reader frame')
    expect(worker.messages[0]).toMatchObject({ config: '[ui]\nicons="unicode"', page: 'https://example.test/reader', oauth: null, relayUrl: null })
    expect(saves).toEqual(['updated config'])
    await expect(client.flushConfig()).rejects.toThrow('storage unavailable')
    expect(errors[0].message).toBe('storage unavailable')
    client.stop()
  })

  test('serializes asynchronous configuration writes', async () => {
    const worker = new FakeWorker(false)
    const saved: string[] = []
    let finishFirst: () => void
    const first = new Promise<void>((resolve) => { finishFirst = resolve })
    const client = new ReaderClient({
      worker: worker as unknown as Worker,
      onConfig: async (text) => {
        if (text === 'first') await first
        saved.push(text)
      },
    })
    const tick = client.tick()
    await Promise.resolve()
    worker.dispatchEvent(new MessageEvent('message', { data: { type: 'config', text: 'first' } }))
    worker.dispatchEvent(new MessageEvent('message', { data: { type: 'config', text: 'second' } }))
    worker.reply()
    await tick
    expect(saved).toEqual([])
    finishFirst()
    await client.flushConfig()
    expect(saved).toEqual(['first', 'second'])
    client.stop()
  })
})

describe('sign-in redirect', () => {
  test('canonical page address is origin plus / or /web/ for the five paths', () => {
    const paths = ['/', '/index.html', '/web', '/web/', '/web/index.html']
    const at = (origin: string, path: string) => canonicalPageAddress(`${origin}${path}?code=1&state=2#done`)
    expect(paths.map((path) => at('https://socli.sh', path))).toEqual([
      'https://socli.sh/',
      'https://socli.sh/',
      'https://socli.sh/web/',
      'https://socli.sh/web/',
      'https://socli.sh/web/',
    ])
    expect(paths.map((path) => at('http://127.0.0.1:5174', path))).toEqual([
      'http://127.0.0.1:5174/',
      'http://127.0.0.1:5174/',
      'http://127.0.0.1:5174/web/',
      'http://127.0.0.1:5174/web/',
      'http://127.0.0.1:5174/web/',
    ])
  })

  test('vite base prefixes the same two paths and other paths stay put', () => {
    expect(canonicalPageAddress('https://socli.sh/web', '/')).toBe('https://socli.sh/web/')
    expect(canonicalPageAddress('https://socli.sh/preview', '/preview/')).toBe('https://socli.sh/preview/')
    expect(canonicalPageAddress('https://socli.sh/preview/index.html', '/preview/')).toBe('https://socli.sh/preview/')
    expect(canonicalPageAddress('https://socli.sh/preview/web', '/preview/')).toBe('https://socli.sh/preview/web/')
    expect(canonicalPageAddress('https://socli.sh/preview/web/index.html?code=1', '/preview/')).toBe('https://socli.sh/preview/web/')
    expect(canonicalPageAddress('https://socli.sh/docs/login.html?x=1#y', '/')).toBe('https://socli.sh/docs/login.html')
    expect(canonicalPageAddress('https://socli.sh/web', '/preview/')).toBe('https://socli.sh/web')
  })

  test('x, twitter and /oauth/authorize stay in this tab; dev/null and non-https do not', () => {
    expect(isAuthorize('https://x.com/i/oauth2/authorize?client_id=1&state=2')).toBe(true)
    expect(isAuthorize('https://twitter.com/i/oauth2/authorize')).toBe(true)
    expect(isAuthorize('https://threads.com/oauth/authorize')).toBe(true)
    expect(isAuthorize('https://mastodon.social/oauth/authorize?client_id=1')).toBe(true)
    expect(isAuthorize('https://social.example/oauth/authorize')).toBe(true)
    expect(isAuthorize('https://not-x.example/i/oauth2/authorize')).toBe(false)
    expect(isAuthorize('https://dev-null.sh/mobile/authorize?user_code=1')).toBe(false)
    expect(isAuthorize('http://x.com/i/oauth2/authorize')).toBe(false)
    expect(isAuthorize('http://threads.com/oauth/authorize')).toBe(false)
    expect(isAuthorize('file:///oauth/authorize')).toBe(false)
    expect(isAuthorize('javascript:alert(1)')).toBe(false)
    expect(isAuthorize('data:text/html,hi')).toBe(false)
  })
})
