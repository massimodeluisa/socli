import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import worker, { badgeLoopback, cliLoopback, fallbackHtml, readerAsset, readerAssetPath, relayTarget, useOrigin } from "./index.js"
import { escapeHtml, inline, pagesModule, renderMarkdown } from "./render.mjs"

test("the bundled pages quote the policy and do not invent a company", () => {
  const privacy = fallbackHtml("/privacy.html")
  const terms = fallbackHtml("/terms.html")
  const deletion = fallbackHtml("/data-deletion.html")
  const home = fallbackHtml("/")
  assert.equal(fallbackHtml("/terms"), terms)
  assert.match(privacy, /Massimo De Luisa/)
  assert.match(terms, /does not store a sign-in/)
  assert.match(terms, /not written to a database/)
  assert.match(terms, /MIT license/)
  assert.match(privacy, /socli\.sh\/threads\/token/)
  assert.match(privacy, /local storage/)
  assert.match(privacy, /does not keep a copy/)
  assert.match(deletion, /socli\.sh\/threads\/deletion/)
  assert.match(deletion, /socli\.sh\/threads\/delete/)
  assert.match(home, /SoCLI/)
  for (const html of [privacy, terms, deletion, home]) {
    assert.match(html, /github\.com\/massimodeluisa\/socli\/issues\/new/)
    assert.doesNotMatch(html, /massimodeluisa@me\.com/)
    assert.doesNotMatch(html, /Smart Squad|ClickRisorse|mybotland|Standard Contractual|Binding Corporate|Essential cookies/)
    assert.doesNotMatch(html, /<script/i)
  }
  assert.equal(fallbackHtml("/threads/token"), null)
  assert.equal(useOrigin(200), true)
  assert.equal(useOrigin(302), true)
  assert.equal(useOrigin(404), false)
  assert.equal(useOrigin(522), false)
})

test("a terminal oauth return bounces to the loopback, and a page visit does not", async () => {
  const xState = `x.${"a".repeat(64)}`
  const thState = `th.${"b".repeat(64)}`
  const code = "abc.def~_+/-"
  const x = new URL(cliLoopback(new URL(`https://socli.sh/?code=${encodeURIComponent(code)}&state=${xState}`)))
  assert.equal(`${x.origin}${x.pathname}`, "http://127.0.0.1:8741/callback")
  assert.equal(x.searchParams.get("code"), "abc.def~_+/-")
  assert.equal(x.searchParams.get("state"), xState)
  const th = new URL(cliLoopback(new URL(`https://socli.sh/index.html?code=code&state=${thState}`)))
  assert.equal(`${th.origin}${th.pathname}`, "http://127.0.0.1:8742/threads/callback")
  assert.equal(th.searchParams.get("code"), "code")
  assert.equal(th.searchParams.get("state"), thState)
  const denied = new URL(
    cliLoopback(new URL(`https://socli.sh/?error=access_denied&error_description=no%20thanks&state=${xState}`)),
  )
  assert.equal(denied.searchParams.get("error"), "access_denied")
  assert.equal(denied.searchParams.get("error_description"), "no thanks")
  assert.equal(denied.searchParams.get("code"), null)
  const longDesc = cliLoopback(
    new URL(`https://socli.sh/?error=access_denied&error_description=${"d".repeat(201)}&state=${xState}`),
  )
  assert.equal(new URL(longDesc).searchParams.get("error_description"), null)
  const brokenDesc = cliLoopback(
    new URL(`https://socli.sh/?error=access_denied&error_description=bad%0Aline&state=${xState}`),
  )
  assert.equal(new URL(brokenDesc).searchParams.get("error_description"), null)
  assert.equal(cliLoopback(new URL(`https://socli.sh/?code=abc&state=${"a".repeat(64)}`)), null)
  assert.equal(cliLoopback(new URL(`https://socli.sh/?code=abc&state=x.${"a".repeat(62)}`)), null)
  assert.equal(cliLoopback(new URL(`https://socli.sh/?code=abc&state=x.${"a".repeat(19)}`)), null)
  assert.equal(cliLoopback(new URL(`https://socli.sh/?code=abc&state=x.${"a".repeat(81)}`)), null)
  assert.equal(cliLoopback(new URL(`https://socli.sh/?code=${"x".repeat(513)}&state=${xState}`)), null)
  assert.equal(cliLoopback(new URL(`https://socli.sh/?state=${xState}`)), null)
  assert.equal(cliLoopback(new URL(`https://socli.sh/privacy?code=abc&state=${xState}`)), null)
  const response = await worker.fetch(new Request(`https://socli.sh/?code=abc&state=${thState}`))
  assert.equal(response.status, 302)
  assert.equal(response.headers.get("cache-control"), "no-store")
  assert.equal(response.headers.get("referrer-policy"), "no-referrer")
  assert.equal(
    new URL(response.headers.get("location")).origin + new URL(response.headers.get("location")).pathname,
    "http://127.0.0.1:8742/threads/callback",
  )
  const head = await worker.fetch(new Request(`https://socli.sh/?code=abc&state=${xState}`, { method: "HEAD" }))
  assert.equal(head.status, 302)
  assert.equal(new URL(head.headers.get("location")).pathname, "/callback")
  const posted = await worker.fetch(new Request(`https://socli.sh/?code=abc&state=${xState}`, { method: "POST" }))
  assert.equal(posted.status, 405)
  assert.equal(posted.headers.get("location"), null)
})

test("a badge oauth return bounces to that private address, and a public one is dropped", async () => {
  const nonce = "a".repeat(64)
  const state = `xb.ac140a04.${nonce}`
  const target = new URL(badgeLoopback(new URL(`https://socli.sh/?code=${encodeURIComponent("abc.def~_+/-")}&state=${state}`)))
  assert.equal(`${target.origin}${target.pathname}`, "http://172.20.10.4:8080/x/callback")
  assert.equal(target.searchParams.get("code"), "abc.def~_+/-")
  assert.equal(target.searchParams.get("state"), state)
  assert.equal(target.port, "8080")
  assert.equal(cliLoopback(new URL(`https://socli.sh/?code=abc&state=${state}`)), null)

  const ten = badgeLoopback(new URL(`https://socli.sh/?code=abc&state=xb.0a010203.${nonce}`))
  assert.equal(new URL(ten).hostname, "10.1.2.3")
  const lan = badgeLoopback(new URL(`https://socli.sh/?code=abc&state=xb.c0a80001.${nonce}`))
  assert.equal(new URL(lan).hostname, "192.168.0.1")
  const low = badgeLoopback(new URL(`https://socli.sh/?code=abc&state=xb.ac100001.${nonce}`))
  assert.equal(new URL(low).hostname, "172.16.0.1")
  const high = badgeLoopback(new URL(`https://socli.sh/?code=abc&state=xb.ac1fffff.${nonce}`))
  assert.equal(new URL(high).hostname, "172.31.255.255")
  const link = badgeLoopback(new URL(`https://socli.sh/?code=abc&state=xb.a9fe0101.${nonce}`))
  assert.equal(new URL(link).hostname, "169.254.1.1")

  for (const hex of ["08080808", "7f000001", "00000000", "ac0f0001", "ac200001", "64400001"]) {
    assert.equal(badgeLoopback(new URL(`https://socli.sh/?code=abc&state=xb.${hex}.${nonce}`)), "drop")
  }
  assert.equal(badgeLoopback(new URL(`https://socli.sh/?state=${state}`)), "drop")
  assert.equal(badgeLoopback(new URL(`https://socli.sh/?code=${"x".repeat(513)}&state=${state}`)), "drop")
  assert.equal(badgeLoopback(new URL(`https://socli.sh/privacy?code=abc&state=${state}`)), null)
  assert.equal(badgeLoopback(new URL(`https://socli.sh/?code=abc&state=x.${nonce}`)), null)

  const bounced = await worker.fetch(new Request(`https://socli.sh/?code=abc&state=${state}`))
  assert.equal(bounced.status, 302)
  assert.equal(bounced.headers.get("cache-control"), "no-store")
  assert.equal(new URL(bounced.headers.get("location")).hostname, "172.20.10.4")
  const dropped = await worker.fetch(new Request(`https://socli.sh/?code=abc&state=xb.08080808.${nonce}`))
  assert.equal(dropped.status, 302)
  assert.equal(dropped.headers.get("location"), "https://socli.sh/")
})

test("the api.x.com relay target stays on that host and a loopback on / is unchanged", () => {
  const state = `x.${"c".repeat(64)}`
  const loop = new URL(cliLoopback(new URL(`https://socli.sh/?code=abc&state=${state}`)))
  assert.equal(`${loop.origin}${loop.pathname}`, "http://127.0.0.1:8741/callback")
  assert.equal(loop.searchParams.get("code"), "abc")
  assert.equal(loop.searchParams.get("state"), state)
  assert.equal(
    cliLoopback(new URL(`https://socli.sh/api/relay/api.x.com/2/oauth2/token?code=abc&state=${state}`)),
    null,
  )
  assert.equal(
    relayTarget("https://socli.sh/api/relay/api.x.com/2/oauth2/token?x=1"),
    "https://api.x.com/2/oauth2/token?x=1",
  )
  assert.equal(
    relayTarget(new URL("https://socli.sh/api/relay/api.x.com/2/oauth2/token?x=1")),
    "https://api.x.com/2/oauth2/token?x=1",
  )
  assert.equal(relayTarget("https://evil.example"), null)
  assert.equal(relayTarget("https://user:pass@socli.sh/api/relay/api.x.com/2/oauth2/token?x=1"), null)
  assert.equal(relayTarget(new URL("https://user:pass@socli.sh/api/relay/api.x.com/2/oauth2/token?x=1")), null)
  assert.equal(relayTarget("https://socli.sh/api/relay/api.x.com/../2/oauth2/token?x=1"), null)
  assert.equal(relayTarget("https://socli.sh/api/relay/api.x.com/%2e%2e/secret"), null)
  assert.equal(relayTarget("https://socli.sh/api/relay/api.x.com/a\\b"), null)
  assert.equal(relayTarget("https://socli.sh/api/relay/api.x.com/%5csecret"), null)
  assert.equal(relayTarget("https://socli.sh/api/relay/api.x.com/%40evil"), null)
  assert.equal(relayTarget("https://socli.sh/api/relay/api.x.com/%2f%2fevil"), null)
})

test("markdown escapes markup and keeps real links", () => {
  const html = renderMarkdown("# Hi\n\nA <script> tag and `socli.config`.\n\nSee [Meta](https://www.facebook.com/privacy/policy/).\n")
  assert.match(html, /<h1>Hi<\/h1>/)
  assert.match(html, /&lt;script&gt;/)
  assert.match(html, /<code>socli\.config<\/code>/)
  assert.match(html, /href="https:\/\/www\.facebook\.com\/privacy\/policy\/"/)
  assert.equal(inline("[x](javascript:alert(1))").includes("javascript:"), true)
  assert.equal(inline("[x](javascript:alert(1))").includes("<a "), false)
  assert.equal(escapeHtml(`"&`), "&quot;&amp;")
})

test("the reader is an asset and a terminal return still leaves the homepage", async () => {
  assert.equal(readerAssetPath("/"), "/index.html")
  assert.equal(readerAssetPath("/index.html"), "/index.html")
  assert.equal(readerAssetPath("/web"), "/web/index.html")
  assert.equal(readerAssetPath("/web/"), "/web/index.html")
  assert.equal(readerAssetPath("/web/index.html"), "/web/index.html")
  assert.equal(readerAssetPath("/docs"), "/docs/index.html")
  assert.equal(readerAssetPath("/wasm/socli.wasm"), "/wasm/socli.wasm")
  assert.equal(readerAssetPath("/assets/app.js"), "/assets/app.js")
  assert.equal(readerAssetPath("/fonts/GeistMono-Regular.otf"), "/fonts/GeistMono-Regular.otf")
  assert.equal(readerAssetPath("/favicon.ico"), "/favicon.ico")
  assert.equal(readerAssetPath("/install.sh"), "/install.sh")
  assert.equal(readerAsset("/privacy"), false)
  assert.equal(readerAsset("/terms.html"), false)
  assert.equal(readerAsset("/data-deletion"), false)
  assert.equal(readerAssetPath("/api/relay/api.x.com/2/users/me"), null)
  assert.equal(readerAssetPath("/threads/token"), null)

  const pageState = "n".repeat(64)
  let fetched = ""
  const env = {
    ASSETS: {
      async fetch(request) {
        fetched = new URL(request.url).pathname
        return new Response("<!doctype html><title>reader</title>", {
          status: 200,
          headers: { "content-type": "text/html; charset=utf-8" },
        })
      },
    },
  }
  const page = await worker.fetch(new Request(`https://socli.sh/web/?code=abc&state=${pageState}`), env)
  assert.equal(page.status, 200)
  assert.equal(page.headers.get("location"), null)
  assert.equal(page.headers.get("content-security-policy"), null)
  assert.equal(fetched, "/web/index.html")
  assert.match(await page.text(), /reader/)

  fetched = ""
  const home = await worker.fetch(new Request(`https://socli.sh/?code=abc&state=${pageState}`), env)
  assert.equal(home.status, 200)
  assert.equal(home.headers.get("content-security-policy"), null)
  assert.equal(
    home.headers.get("strict-transport-security"),
    "max-age=31536000; includeSubDomains",
  )
  assert.equal(fetched, "/index.html")

  fetched = ""
  const bounced = await worker.fetch(
    new Request(`https://socli.sh/?code=abc&state=x.${"a".repeat(64)}`),
    env,
  )
  assert.equal(bounced.status, 302)
  assert.equal(fetched, "")
  assert.match(bounced.headers.get("location"), /^http:\/\/127\.0\.0\.1:8741\/callback/)

  fetched = ""
  const terms = await worker.fetch(new Request("https://socli.sh/terms"), env)
  assert.equal(fetched, "")
  assert.match(await terms.text(), /does not store a sign-in/)
  assert.match(terms.headers.get("content-security-policy"), /default-src 'none'/)

  const posted = await worker.fetch(new Request("https://socli.sh/web/", { method: "POST" }), env)
  assert.equal(posted.status, 405)
  assert.equal(fetched, "")

  const unbound = await worker.fetch(new Request("https://socli.sh/"))
  assert.equal(unbound.status, 200)
  assert.match(unbound.headers.get("content-security-policy"), /default-src 'none'/)
  assert.match(await unbound.text(), /SoCLI/)
})

test("pages.js matches the markdown", () => {
  const current = readFileSync(new URL("./pages.js", import.meta.url), "utf8")
  assert.equal(current, pagesModule())
  const source = readFileSync(new URL("../../website/privacy.md", import.meta.url), "utf8")
  assert.match(source, /GDPR Article 6\(1\)\(b\)/)
  assert.doesNotMatch(source, /Standard Contractual|nonprofit/)
})
