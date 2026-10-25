# Changelog

Release notes are kept here. Move entries from `[Unreleased]` into a dated version section before tagging a release.

## [Unreleased]

- Fixed terminal and web sign-in for X, Bluesky, Threads, Mastodon, and dev/null.
- Served the web reader from the site worker and relayed only api.x.com.
- Updated tokio to 1.54.1 and rustls-webpki to 0.103.16.
- Added CI, Dependabot, and cargo-deny.
- Added tag-triggered native archives, Linux packages, a WebAssembly npm package, Homebrew updates, and website deployment.
- Added an npm launcher with platform packages for native `npx` installation and verified publishing retries.
- Added soclib with mounted and headless browser readers, isolated instances, host adapters, bundled assets, and state-preserving theme changes.
