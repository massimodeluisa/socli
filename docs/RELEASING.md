# Releasing SoCLI

Pushing a release tag is the only event that publishes SoCLI. A manual Release workflow run verifies the current source and builds artifacts without publishing. `.github/workflows/release.yml` builds the tagged commit, publishes its GitHub release and npm packages, updates a Homebrew tap when configured, and deploys the website. Branch pushes and pull requests run `.github/workflows/ci.yml` without publishing. The web workflow is reusable and has no branch, tag, or manual trigger of its own.

## Release outputs

Every release builds the following targets. Linux builds use musl so the distributed executable does not require the destination's glibc version. ARM means ARMv7 with hardware floating point; ARM64 means AArch64. x86 and x64 mean 32-bit and 64-bit Intel-compatible Linux respectively.

| Platform | Rust target | Packages |
| --- | --- | --- |
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz`, `.dmg` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz`, `.dmg` |
| Linux ARMv7 | `armv7-unknown-linux-musleabihf` | `.tar.gz`, `.deb`, `.rpm`, `.apk` |
| Linux ARM64 | `aarch64-unknown-linux-musl` | `.tar.gz`, `.deb`, `.rpm`, `.apk` |
| Linux x86 | `i686-unknown-linux-musl` | `.tar.gz`, `.deb`, `.rpm`, `.apk` |
| Linux x64 | `x86_64-unknown-linux-musl` | `.tar.gz`, `.deb`, `.rpm`, `.apk` |
| Browser | `wasm32-unknown-unknown` | Website `.tar.gz`, npm `.tgz` |

The Windows targets `i686-pc-windows-msvc` and `x86_64-pc-windows-msvc` remain commented out in the matrix. Enabling them also requires adapting packaging and validating authentication and terminal behavior on Windows.

`website/public/install.sh` is served at `https://socli.sh/install.sh`. It selects these package names for the machine, verifies `SHA256SUMS`, and installs. Arch and Omarchy receive the musl archive. The site worker allows `/install.sh` as a static asset.

The GitHub release includes every package, `socli.rb`, the source-build candidate `socli-core.rb`, a source archive, and `SHA256SUMS`. The release is rejected when any of those packages is missing, including both macOS disk images, the four Debian packages, the RPM and Alpine packages, both npm archives, and the Homebrew formula. Required artifacts must be nonempty regular files, and npm package-name overrides apply to the completeness check. Native archives contain the executable, README, changelog, license, and third-party notices. Linux packages install the executable under `/usr/bin` and include the license and notices. nFPM translates the matrix architecture into each package format's architecture name. Both macOS builds target macOS 12 or newer and include the embedded Swift authentication helper. Each disk image contains `socli.pkg`, which installs `socli` into `/usr/local/bin`. The package is not notarized.

The native npm package is `soclish`. It exposes the `soclish` and `socli` commands, so both `npx soclish@latest --demo` and `bunx soclish@latest --demo` run the native reader after publication. A global npm installation puts both names on your command path. The [CLI package README](../packages/cli/README.md) covers installation and supported platforms.

The `soclish` package includes all six native executables under `native/`, with one SHA-256 entry for each platform. The launcher selects the matching bundled executable and verifies its hash before starting it with inherited terminal input and output. There are no architecture-specific npm packages or runtime npm dependencies. Installation downloads all six binaries together.

The browser npm package is `@massimodeluisa/soclib`. It includes the WebAssembly engine, the shared reader worker and renderer, styles, fonts, and TypeScript declarations. Its `mountReader` export embeds a reader in a supplied HTML element; `createReader` returns frames for a custom renderer. The [library README](../packages/web/README.md) documents both APIs and the host callbacks. The website archive contains the complete browser application and uses the same optimized WebAssembly bytes as soclib and the Pages deployment. The default npm names require ownership before publication; the name variables can override them.

## Configure publishing

Complete these settings in the GitHub repository before pushing a release tag. The workflow validates the package maintainer for every release. If `HOMEBREW_TAP` is set, it also validates that destination and its publishing credentials before building a stable release.

| Setting | Kind | Purpose |
| --- | --- | --- |
| `PACKAGE_MAINTAINER` | Actions variable | The Linux package maintainer, written as a real name and email address. |
| `HOMEBREW_TAP` | Actions variable, optional | An existing, initialized tap repository, written as `owner/repository`. Leave unset when distributing through Homebrew Core. |
| `HOMEBREW_TAP_SSH_KEY` | Actions secret, required with the project tap | A deploy key with write access restricted to the tap repository. |
| `HOMEBREW_TAP_TOKEN` | Actions secret, alternative | A token with Contents write access to the tap. The source repository's `GITHUB_TOKEN` cannot write to another repository. |
| `NPM_PACKAGE_NAME` | Actions variable, optional | Overrides the npm package name in `packages/web/package.json`. |
| `NPM_CLI_PACKAGE_NAME` | Actions variable, optional | Overrides the launcher name in `packages/cli/package.json` without changing the native command name. |
| `NPM_TOKEN` | Actions secret, optional | A granular publishing token for bootstrapping or when npm trusted publishing is unavailable. |
| `SITE_DEPLOY_TARGET` | Actions variable | `cloudflare` for socli.sh; other values retain GitHub Pages deployment. |
| `CLOUDFLARE_API_TOKEN` | Actions secret, required for Cloudflare | A deployment token scoped to the existing site worker account and socli.sh routes. |
| `SITE_BASE` | Actions variable, optional | The website's deployment path; the default is `/`. |

For npm, prefer [trusted publishing](https://docs.npmjs.com/trusted-publishers/). Configure each package's trusted publisher to use this GitHub repository and `release.yml`, and allow direct `npm publish`. There are exactly two npm packages: `soclish` and `@massimodeluisa/soclib`. The publishing job uses Node 24 and has `id-token: write`. Each generated package's repository URL comes from `GITHUB_REPOSITORY`, so it matches the publishing repository. If you cannot configure trusted publishing for the first publication, bootstrap with an `NPM_TOKEN` authorized for both package names, then configure trust for each package and remove the token.

The production site uses the existing `socli-site` Cloudflare Worker. Set `SITE_DEPLOY_TARGET=cloudflare` and provide `CLOUDFLARE_API_TOKEN`. The deployment extracts the checksummed website archive from the release and deploys the tagged worker source with pinned Wrangler. Keep the configured routes, including the longer Threads token routes, and do not replace them with a custom domain. A local Wrangler OAuth login does not supply a GitHub Actions credential.

For a GitHub Pages destination, set GitHub Pages to deploy through GitHub Actions. The `github-pages` environment must allow release tags. The workflow's `GITHUB_TOKEN` receives Contents write only in the GitHub release job; Pages and npm receive their own deployment permissions.

Protect `main` with CI checks and protect release tags with a tag ruleset. Use annotated tags, or signed tags when signing is configured. Treat a published tag as permanent: a correction gets a new version.

## Prepare and publish a version

1. Finish the changes on a branch and merge them into `main` after CI passes. Keep entries under `[Unreleased]` in `CHANGELOG.md` while developing.
2. Set the version in `Cargo.toml`, `packages/cli/package.json`, and `packages/web/package.json`. Update the root `socli` entry in `Cargo.lock` to the same version while preserving dependency versions, then review the lockfile diff. Keep all four versions equal.
3. Move the release's changelog entries into `## [X.Y.Z] - YYYY-MM-DD`, using the actual release date. Leave `[Unreleased]` for subsequent changes. The dated section needs at least one entry and becomes the GitHub release body.
4. Commit the release preparation through your normal review process and merge it into `main`. Check out that merged commit and run `bun scripts/release.ts validate vX.Y.Z` locally, substituting the actual version.
5. Create an annotated `vX.Y.Z` tag on the prepared commit and push that tag to the GitHub publishing repository. Confirm the destination with `git remote -v`: a proxy remote is not evidence that GitHub received the push.
6. Follow the Release workflow. Validation rejects malformed tags, version mismatches, missing release notes, and commits outside `main`'s history. A stable version must be newer than the published stable versions returned by GitHub. Tests run before the native and web builds.

The workflow uses the Rust toolchain pinned by `rust-toolchain.toml`, locked Cargo dependencies, and the frozen website lockfile. It builds and runs `--version` for each native target, using `cross` to build Linux and QEMU user-mode emulation to run the ARM and 32-bit executables. Linux smoke tests also read the offline demo feed. Native builds and the web build must all succeed before the GitHub release becomes public. The npm job publishes the exact `.tgz` files already attached to the GitHub release. The CLI contains its native executables, so publication does not depend on separate platform packages. A configured Homebrew tap updates independently of npm. The selected website destination deploys after npm succeeds and the tap update succeeds or is skipped.

The dated changelog section must describe the exact tagged source.

## Prereleases and failures

Prerelease tags use `vX.Y.Z-alpha.N`, `vX.Y.Z-beta.N`, or `vX.Y.Z-rc.N`, with the same version in the Cargo manifest, both npm manifests, and `Cargo.lock`. They create a GitHub prerelease and publish npm with the `next` tag. They do not change the Homebrew tap, stable website, or GitHub's latest-release selection. Stable tags publish npm with `latest` and update all stable destinations.

Release runs share a concurrency group so publishing jobs from different versions do not interleave. External registries do not provide a transaction across GitHub, npm, Homebrew, and Pages. A failure after GitHub publication can therefore leave some destinations updated and others pending. Fix the external configuration and rerun the failed jobs in the existing workflow run. Rerunning all jobs for an already-published stable tag fails release-order validation. Do not move the tag or overwrite published binaries to repair a release.

A workflow fix on `main` applies to future tags. Published tags retain their original workflow. If the tagged npm publisher itself has a bug, download and verify the existing release assets, then use the corrected publishing helper with those exact tarballs after signing in. Do not move the tag or regenerate the published packages.

If npm fails partway through publishing, the retry checks every existing version's registry integrity against the local tarball. Identical packages are skipped; a version with different bytes stops the job. Authentication and registry errors also stop the job rather than being treated as missing packages.

When a tap is configured, the Homebrew job writes `Formula/socli.rb` on its default branch. The formula selects the matching macOS or 64-bit Linux archive and verifies its SHA-256. With the deploy key, a concurrent change makes the Git push fail. Token-based updates use GitHub's file SHA check. The tap needs an initialized branch before its first update.

The project tap is `massimodeluisa/homebrew-tap`. Install it once with `brew tap massimodeluisa/tap`, then use `brew install socli` and `brew upgrade socli`. Its deploy key can write only to that repository. GitHub's SSH host keys are fetched through its HTTPS API before connecting.

Homebrew Core accepts source builds, so releases also contain `socli-core.rb` and the checksummed source archive. Rename that candidate to `Formula/s/socli.rb` in a Core contribution checkout, build it from source, run `brew test socli` and `brew audit --strict --new --online socli`, and review the formula before submitting it. The candidate uses locked Cargo dependencies and tests the offline feed without signing in.

Core submission is pending eligibility under the [package acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy). A repository under 30 days old is normally ineligible. Owner submissions normally require at least 90 forks, 90 watchers, or 225 stars. The project had none of those on 2026-10-25. Acceptance remains a maintainer decision. Homebrew's [contribution instructions](https://docs.brew.sh/How-To-Open-a-Homebrew-Pull-Request) also require the contributor to review AI-assisted work, disclose that assistance, and answer review questions personally.

Debian, RPM, and Alpine packages are published as GitHub release assets. A person downloads the file for that machine and installs it with `apt`, `dnf`, or `apk`. There is no public archive that accepts a project release and then serves it to every `apt upgrade`, `dnf upgrade`, and `apk upgrade` with no further setup. Debian, Fedora, and Alpine only upgrade packages that are already in their own archives, and a release tag cannot upload into those archives. A third-party repository can track later versions after its source is added once. This repository does not configure one. Distribution-maintained packages and Homebrew Core also have their own review and update processes; the workflow updates your tap.

## Why this workflow

[ripgrep's release workflow](https://github.com/BurntSushi/ripgrep/blob/master/.github/workflows/release.yml) demonstrates tag-only releases, manifest checks, a target matrix, `cross`, archives, and Debian packages. [bat's workflow](https://github.com/sharkdp/bat/blob/master/.github/workflows/CICD.yml) combines branch CI with a release gate and packages builds for several architectures. SoCLI separates CI from publishing so the events that can publish remain explicit.

[cargo-dist](https://github.com/axodotdev/cargo-dist) can generate a release pipeline with npm and Homebrew installers. It is useful when its generated installers and target support match the project. SoCLI currently uses an explicit matrix because the release also includes two 32-bit Linux targets, Linux package formats, and a browser engine that shares the website build. [nFPM](https://github.com/goreleaser/nfpm/blob/main/www/content/docs/configuration.md) packages the existing Linux executables without rebuilding them.

A release-preparation tool such as [release-plz](https://release-plz.dev/) could later prepare version and changelog pull requests. Tag creation would still be the publishing boundary. Until then, version preparation is reviewed on `main`, and the workflow checks that each destination receives the tagged version.
