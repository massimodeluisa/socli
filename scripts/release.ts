import { appendFile, chmod, copyFile, cp, lstat, mkdir, readdir, readFile, writeFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { basename, join } from 'node:path'
import { execFileSync } from 'node:child_process'

export function releaseVersion(tag: string): { version: string; prerelease: boolean } {
  const match = /^v((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-(?:alpha|beta|rc)\.(?:0|[1-9]\d*))?)$/.exec(tag)
  if (!match) throw new Error('release tag must be vX.Y.Z or vX.Y.Z-{alpha,beta,rc}.N')
  return { version: match[1], prerelease: match[1].includes('-') }
}

export function releaseNotes(changelog: string, version: string): string {
  const sections = changelog.split(/^## /m).slice(1)
  const matching = sections.filter((value) => value.startsWith(`[${version}] - `))
  if (matching.length !== 1) throw new Error(`CHANGELOG.md needs exactly one dated [${version}] section`)
  const section = matching[0]
  const [heading, ...body] = section.split('\n')
  const date = heading.slice(`[${version}] - `.length).trim()
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || new Date(`${date}T00:00:00Z`).toISOString().slice(0, 10) !== date) {
    throw new Error('changelog release date must be YYYY-MM-DD')
  }
  const notes = body.join('\n').trim()
  if (!notes || !/^[-*] .+/m.test(notes)) throw new Error('release notes need at least one changelog entry')
  return `${notes}\n`
}

export function validateVersions(version: string, cargo: string, lock: string, ...npm: string[]): void {
  const manifest = Bun.TOML.parse(cargo)
  const locked = Bun.TOML.parse(lock)
  const root = locked.package.find((item) => item.name === manifest.package.name && !item.source)
  if (manifest.package.version !== version || root?.version !== version || npm.some((value) => JSON.parse(value).version !== version)) {
    throw new Error('tag, Cargo.toml, Cargo.lock and both npm package versions must match')
  }
}

export function validateReleaseOrder(version: string, releases: Array<{ tagName: string; isDraft: boolean; isPrerelease: boolean }>): void {
  if (version.includes('-')) return
  const parts = version.split('.').map(BigInt)
  for (const release of releases) {
    if (release.isDraft || release.isPrerelease || !/^v\d+\.\d+\.\d+$/.test(release.tagName)) continue
    const previous = release.tagName.slice(1).split('.').map(BigInt)
    const difference = parts.findIndex((value, index) => value !== previous[index])
    if (difference === -1 || parts[difference] < previous[difference]) {
      throw new Error(`stable release must be newer than ${release.tagName}`)
    }
  }
}

async function digest(path: string): Promise<string> {
  return createHash('sha256').update(await readFile(path)).digest('hex')
}

export async function homebrewFormula(version: string, repository: string, assets: string): Promise<string> {
  const archive = (target) => `socli-${version}-${target}.tar.gz`
  const platforms = [
    ['macos', 'arm', 'aarch64-apple-darwin'],
    ['macos', 'intel', 'x86_64-apple-darwin'],
    ['linux', 'arm', 'aarch64-unknown-linux-musl'],
    ['linux', 'intel', 'x86_64-unknown-linux-musl'],
  ]
  const blocks = await Promise.all(['macos', 'linux'].map(async (os) => {
    const architectures = await Promise.all(platforms.filter(([platform]) => platform === os).map(async ([, cpu, target]) => {
      const name = archive(target)
      return `    on_${cpu} do\n      url "https://github.com/${repository}/releases/download/v${version}/${name}"\n      sha256 "${await digest(join(assets, name))}"\n    end`
    }))
    return `  on_${os} do\n${architectures.join('\n\n')}\n  end`
  }))
  return `class Socli < Formula
  desc "Thread-first terminal client for social networks"
  homepage "https://socli.sh"
  license "MIT"

${blocks.join('\n\n')}

  def install
    bin.install "socli"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/socli --version")
    posts = JSON.parse(shell_output("#{bin}/socli feed --demo --json"))
    assert posts.any? { |post| post.fetch("network") == "devnull" }
  end
end
`
}

export async function homebrewSourceFormula(version: string, repository: string, archive: string): Promise<string> {
  return `class Socli < Formula
  desc "Thread-first terminal client for social networks"
  homepage "https://socli.sh"
  url "https://github.com/${repository}/archive/refs/tags/v${version}.tar.gz"
  sha256 "${await digest(archive)}"
  license "MIT"

  depends_on "rust" => :build

  on_macos do
    depends_on xcode: ["13.0", :build]
  end

  def fetch
    system "cargo", "fetch", *std_cargo_fetch_args
  end

  def install
    system "cargo", "install", *std_cargo_args, "--bin", "socli"
  end

  test do
    posts = JSON.parse(shell_output("#{bin}/socli feed --demo --json"))
    assert posts.any? { |post| post.fetch("network") == "devnull" }
  end
end
`
}

async function validate(tag: string) {
  const { version, prerelease } = releaseVersion(tag)
  if (process.env.PUBLISHED_RELEASES) validateReleaseOrder(version, JSON.parse(await readFile(process.env.PUBLISHED_RELEASES, 'utf8')))
  validateVersions(version, await readFile('Cargo.toml', 'utf8'), await readFile('Cargo.lock', 'utf8'), await readFile('packages/web/package.json', 'utf8'), await readFile('packages/cli/package.json', 'utf8'))
  const notes = releaseNotes(await readFile('CHANGELOG.md', 'utf8'), version)
  await mkdir('release', { recursive: true })
  await writeFile('release/notes.md', notes)
  if (process.env.GITHUB_OUTPUT) {
    await appendFile(process.env.GITHUB_OUTPUT, `version=${version}\nprerelease=${prerelease}\nnpm_tag=${prerelease ? 'next' : 'latest'}\n`)
  }
  console.log(`validated ${tag}`)
}

async function webPackage(wasm: string, output: string) {
  const cargo = Bun.TOML.parse(await readFile('Cargo.toml', 'utf8')).package
  const manifest = JSON.parse(await readFile('packages/web/package.json', 'utf8'))
  if (manifest.version !== cargo.version) throw new Error('web package version must match Cargo.toml')
  if (process.env.NPM_PACKAGE_NAME) manifest.name = process.env.NPM_PACKAGE_NAME
  const repository = process.env.GITHUB_REPOSITORY
  if (repository) manifest.repository.url = `git+https://github.com/${repository}.git`
  const bytes = await readFile(wasm)
  if (!WebAssembly.validate(bytes)) throw new Error('web package input is not a valid WebAssembly module')
  await mkdir(output, { recursive: true })
  await writeFile(join(output, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  await copyFile(wasm, join(output, 'socli.wasm'))
  for (const file of ['index.d.ts', 'README.md']) await copyFile(join('packages/web', file), join(output, file))
  await cp('packages/web/fonts', join(output, 'fonts'), { recursive: true })
  await writeFile(join(output, 'reader.css'), `${await readFile('packages/web/view.css', 'utf8')}\n${await readFile('packages/web/renderer.css', 'utf8')}`)
  for (const [entrypoint, name] of [['packages/web/index.js', 'index.js'], ['website/.vitepress/theme/reader.worker.ts', 'reader.worker.js']]) {
    const result = await Bun.build({ entrypoints: [entrypoint], outdir: output, target: 'browser', format: 'esm', naming: name, define: { 'import.meta.env.BASE_URL': JSON.stringify('/') } })
    if (!result.success) throw new AggregateError(result.logs, `could not bundle ${name}`)
  }
  for (const file of ['LICENSE', 'THIRD-PARTY-NOTICES.md']) await copyFile(file, join(output, file))
}

export const npmPlatforms = [
  { key: 'darwin-x64', target: 'x86_64-apple-darwin', os: 'darwin', cpu: 'x64' },
  { key: 'darwin-arm64', target: 'aarch64-apple-darwin', os: 'darwin', cpu: 'arm64' },
  { key: 'linux-arm', target: 'armv7-unknown-linux-musleabihf', os: 'linux', cpu: 'arm' },
  { key: 'linux-arm64', target: 'aarch64-unknown-linux-musl', os: 'linux', cpu: 'arm64' },
  { key: 'linux-ia32', target: 'i686-unknown-linux-musl', os: 'linux', cpu: 'ia32' },
  { key: 'linux-x64', target: 'x86_64-unknown-linux-musl', os: 'linux', cpu: 'x64' },
]

export async function nativeNpmPackages(assets: string, output: string) {
  const manifest = JSON.parse(await readFile('packages/cli/package.json', 'utf8'))
  const version = Bun.TOML.parse(await readFile('Cargo.toml', 'utf8')).package.version
  releaseVersion(`v${version}`)
  if (manifest.version !== version) throw new Error('CLI package version must match Cargo.toml')
  if (process.env.NPM_CLI_PACKAGE_NAME) manifest.name = process.env.NPM_CLI_PACKAGE_NAME
  if (process.env.GITHUB_REPOSITORY) manifest.repository.url = `git+https://github.com/${process.env.GITHUB_REPOSITORY}.git`
  const platforms = {}
  delete manifest.optionalDependencies
  const wrapper = join(output, 'cli')
  await mkdir(join(wrapper, 'bin'), { recursive: true })
  for (const platform of npmPlatforms) {
    const directory = join(wrapper, 'native', platform.key)
    await mkdir(directory, { recursive: true })
    const archive = `socli-${version}-${platform.target}`
    execFileSync('tar', ['-xzf', join(assets, `${archive}.tar.gz`), '-C', directory, '--strip-components=1', `${archive}/socli`])
    const binary = join(directory, 'socli')
    await chmod(binary, 0o755)
    platforms[platform.key] = { path: `native/${platform.key}/socli`, sha256: await digest(binary) }
  }
  await writeFile(join(wrapper, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  await writeFile(join(wrapper, 'platforms.json'), `${JSON.stringify(platforms, null, 2)}\n`)
  await copyFile('packages/cli/bin/socli.js', join(wrapper, 'bin/socli.js'))
  await chmod(join(wrapper, 'bin/socli.js'), 0o755)
  await copyFile('packages/cli/README.md', join(wrapper, 'README.md'))
  for (const file of ['LICENSE', 'THIRD-PARTY-NOTICES.md']) await copyFile(file, join(wrapper, file))
}

export function releasePackages(version: string, cliPackageName = 'soclish', webPackageName = '@massimodeluisa/soclib'): string[] {
  const mac = ['aarch64-apple-darwin', 'x86_64-apple-darwin']
  const linux = [
    'aarch64-unknown-linux-musl',
    'armv7-unknown-linux-musleabihf',
    'i686-unknown-linux-musl',
    'x86_64-unknown-linux-musl',
  ]
  return [
    ...mac.flatMap((target) => [`socli-${version}-${target}.tar.gz`, `socli-${version}-${target}.dmg`]),
    ...linux.map((target) => `socli-${version}-${target}.tar.gz`),
    `socli_${version}-1_amd64.deb`,
    `socli_${version}-1_arm64.deb`,
    `socli_${version}-1_armhf.deb`,
    `socli_${version}-1_i386.deb`,
    `socli-${version}-1.aarch64.rpm`,
    `socli-${version}-1.armv7hl.rpm`,
    `socli-${version}-1.i386.rpm`,
    `socli-${version}-1.x86_64.rpm`,
    `socli_${version}-r1_aarch64.apk`,
    `socli_${version}-r1_armv7.apk`,
    `socli_${version}-r1_x86.apk`,
    `socli_${version}-r1_x86_64.apk`,
    'socli.rb',
    'socli-core.rb',
    `socli-${version}-source.tar.gz`,
    `socli-${version}-web.tar.gz`,
    `${cliPackageName.replace(/^@/, '').replace('/', '-')}-${version}.tgz`,
    `${webPackageName.replace(/^@/, '').replace('/', '-')}-${version}.tgz`,
  ]
}

export async function requireReleasePackages(assets: string, version: string, cliPackageName = 'soclish', webPackageName = '@massimodeluisa/soclib') {
  const present = new Set(await readdir(assets))
  const names = releasePackages(version, cliPackageName, webPackageName)
  const missing = names.filter((name) => !present.has(name))
  if (missing.length) throw new Error(`release is missing packages:\n${missing.join('\n')}`)
  for (const name of names) {
    const file = await lstat(join(assets, name))
    if (!file.isFile() || file.size === 0) throw new Error(`release package must be a nonempty regular file: ${name}`)
  }
}

async function checksums(assets: string) {
  const files = (await readdir(assets)).filter((name) => name !== 'SHA256SUMS').sort()
  const sums = await Promise.all(files.map(async (name) => `${await digest(join(assets, name))}  ${basename(name)}`))
  await writeFile(join(assets, 'SHA256SUMS'), `${sums.join('\n')}\n`)
}

if (import.meta.main) {
  const [command, ...args] = process.argv.slice(2)
  if (command === 'validate' && args.length === 1) await validate(args[0])
  else if (command === 'web' && args.length === 2) await webPackage(args[0], args[1])
  else if (command === 'npm-native' && args.length === 2) await nativeNpmPackages(args[0], args[1])
  else if (command === 'formula' && args.length === 3) await writeFile(join(args[2], 'socli.rb'), await homebrewFormula(args[0], args[1], args[2]))
  else if (command === 'source-formula' && args.length === 4) await writeFile(args[3], await homebrewSourceFormula(args[0], args[1], args[2]))
  else if (command === 'require-packages' && args.length === 2) await requireReleasePackages(args[1], args[0], process.env.NPM_CLI_PACKAGE_NAME || undefined, process.env.NPM_PACKAGE_NAME || undefined)
  else if (command === 'checksums' && args.length === 1) await checksums(args[0])
  else throw new Error('usage: release.ts validate TAG | web WASM OUT | npm-native ASSETS OUT | formula VERSION REPOSITORY ASSETS | source-formula VERSION REPOSITORY ARCHIVE OUT | require-packages VERSION ASSETS | checksums ASSETS')
}
