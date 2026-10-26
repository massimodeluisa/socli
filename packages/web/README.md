# soclib

soclib embeds the SoCLI reader in a browser application. It runs the same Rust reader as the terminal client, compiled to WebAssembly inside a worker, and paints its cell updates with the website's renderer. The package includes the worker, styles, fonts, TypeScript declarations, and WASM module. Each mounted reader has its own session and shadow root.

The native terminal package is `soclish`, available through `npx soclish` or `bunx soclish` after publication. The tag release workflow publishes both packages at the same version. Until the first release, use the output of the local packaging command below.

## Mount the reader

Install `@massimodeluisa/soclib` in your browser project after publication:

```sh
npm install @massimodeluisa/soclib
```

Give the reader a container with a defined height, then mount it:

```html
<div id="reader" style="height: 600px"></div>
```

```js
import { mountReader } from '@massimodeluisa/soclib'

const reader = await mountReader(document.querySelector('#reader'), {
  theme: 'auto',
  configStore: {
    load: () => localStorage.getItem('my-app.socli') ?? '',
    save: (text) => localStorage.setItem('my-app.socli', text),
  },
})

reader.focus()
await reader.key('ArrowUp')
await reader.key('Enter')
```

A signed-out reader starts at the welcome screen, with X selected. ArrowUp selects the offline demo, and Enter opens it. The embed uses the existing reader's keys, touch gestures, images, and light and dark palettes. It fits its container and observes size changes. Multiple readers can coexist without sharing DOM IDs, configuration, or keyboard focus.

The returned controller has `key(name)` for a character or named key such as `Enter`, `Escape`, or `ArrowDown`; `write(bytes)` for the engine's encoded key bytes; and `paste(text)` for pasted text. Input methods return the resulting frame. `focus()` moves keyboard focus into the reader. `resize()` schedules a refit after a host layout change, and `setTheme('auto' | 'dark' | 'light')` updates the existing session while preserving its state. A theme explicitly selected inside the reader retains its palette choice. `text` exposes the current plain-text screen. Call `dispose()` when removing the component; it stops the worker, timers, listeners, and observers and removes only the library-owned element. Repeated disposal is safe, and later input rejects.

The stylesheet uses `--soclib-font`, `--soclib-font-size`, and `--soclib-line-height` for host typography overrides. The default is Geist Mono at 13px with a 20px line height. Pass `fonts: false` to use a host-provided font, and set `ui.icons` to `unicode` in the config if your font has no Nerd Font glyphs.

## Host callbacks and credentials

Without a `configStore`, configuration lives only in that reader's worker and is lost on disposal. A store's `load()` returns the TOML string used at startup, and `save(text)` receives changes. Both may return promises. Writes are serialized in arrival order; `await reader.flushConfig()` waits for outstanding saves and rejects if the latest save failed. Failures are also reported through `onError`, and a failed save does not block later writes. Saved configuration can include access tokens, so choose storage appropriate for the host application. Readers share persisted configuration only when you give them the same store.

Pass `onOpenUrl(url)` to integrate authentication and external links, `onClipboard(text)` to handle copied text, `onFrame(frame)` to observe screen changes, and `onError(error)` to report storage or rendering failures. The mounted reader otherwise uses the browser clipboard, navigates the current page for authorization URLs, and opens other links in a new window. Browser permission and popup rules still apply.

OAuth callbacks are explicit. Pass `oauth: { code, state }` when your app receives an authorization callback, and restore the same configuration store that started sign-in. `pageUrl` identifies the callback page; it defaults to the current page with its query and fragment removed. The library does not consume or rewrite the host page's query string. Provider app registrations must allow the callback URL used by the host. Builds for your own provider apps can set the public `SOCLI_X_CLIENT_ID` and `SOCLI_THREADS_APP_ID` values when compiling the Rust module.

Browser sessions cannot inherit terminal credential files, browser-session cookies, or the native `ssh-keygen` integration. Provider requests still follow browser cross-origin rules. Pass `relayUrl` to the host's forwarding prefix for `api.x.com`, for example `/api/relay/api.x.com`; the worker appends the upstream path and query. The default is `null`, which sends requests directly. Other provider URLs are not rewritten, and your relay must implement its own upstream allowlist.

## Supply your own renderer

`createReader` starts the worker without mounting a DOM view:

```js
import { createReader } from '@massimodeluisa/soclib'

const reader = await createReader({ cols: 80, rows: 24 })
console.log(reader.text)
await reader.write(Uint8Array.of(0x80))
await reader.write(Uint8Array.of(0x0d))
console.log(reader.frame)
reader.dispose()
```

The encoded key `0x80` is ArrowUp and `0x0d` is Enter. A frame contains the changed cell bytes, plain text, cursor, picture descriptors, clipboard text, and an outgoing URL. Headless hosts handle those side effects themselves. The controller provides `write`, `paste`, `pointer`, `resize`, and `tick`; call `tick` when your renderer needs animation updates. Dimensions must be integers between 1 and 1000. Input is serialized, and a full queue rejects additional requests rather than retaining unlimited work.

## Assets and low-level access

Default asset URLs resolve relative to the installed library. Bundlers that recognize `new URL(..., import.meta.url)` and module workers can relocate them. When your build serves assets elsewhere, pass absolute or page-relative `wasmUrl` and `workerUrl`, plus `styleUrl` for a mounted reader. Serve the files with their correct content types, and permit the worker, WASM compilation, fonts, and styles in your site's content security policy. The font licenses are included under `fonts/`.

`wasmUrl`, `workerUrl`, and `styleUrl` are exported for asset discovery. The package also exports the `@massimodeluisa/soclib/socli.wasm`, `@massimodeluisa/soclib/reader.worker.js`, and `@massimodeluisa/soclib/style.css` subpaths. Type declarations are included at the main entry point.

`instantiate(imports, url)` remains available when you supply the engine's entire host interface yourself. It returns the standard WebAssembly `{ module, instance }` result. The `env` imports are `socli_millis`, `socli_mono`, `socli_http`, `socli_config_read`, `socli_config_write`, `socli_random`, and `socli_page`. HTTP is synchronous inside the engine, so run it in a worker. The mounted and headless APIs already do this. The shared [reader worker](https://github.com/massimodeluisa/socli/blob/main/website/.vitepress/theme/reader.worker.ts) implements the host imports, and the [renderer](https://github.com/massimodeluisa/socli/blob/main/website/.vitepress/theme/wasm-reader.ts) implements the cell view. Use worker and WASM assets from the same release.

## Build and verify locally

From the repository root, build and package the library:

```sh
cargo build --locked --release --no-default-features --target wasm32-unknown-unknown --lib
bun scripts/release.ts web target/wasm32-unknown-unknown/release/socli.wasm /tmp/soclib-package
bun scripts/serve-soclib.ts /tmp/soclib-package
```

Open the printed local address and run the browser checks. They exercise two independent mounted readers, keyboard input, theme changes, resizing, headless frames, and disposal. The release workflow uses the optimized WASM module produced by its web build and publishes the resulting package only from a version tag.
