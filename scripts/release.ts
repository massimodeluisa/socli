import { appendFile, chmod, copyFile, cp, mkdir, readdir, readFile, writeFile } from 'node:fs/promises'
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
  const section = sections.find((value) => value.startsWith(`[${version}] - `))
  if (!section) throw new Error(`CHANGELOG.md needs a dated [${version}] section`)
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
  const blocks = await Promise.all(platforms.map(async ([os, cpu, target]) => {
    const name = archive(target)
    return `  on_${os} do\n    on_${cpu} do\n      url "https://github.com/${repository}/releases/download/v${version}/${name}"\n      sha256 "${await digest(join(assets, name))}"\n    end\n  end`
  }))
  return `class Socli < Formula
  desc "Thread-first terminal client for social networks"
  homepage "https://socli.sh"
  version "${version}"
  license "MIT"

${blocks.join('\n\n')}

  def install
    bin.install "socli"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/socli --version")
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
  await Bun.build({ entrypoints: ['packages/web/index.js'], outdir: output, target: 'browser', format: 'esm', naming: 'index.js', define: { 'import.meta.env.BASE_URL': JSON.stringify('/') } })
  await Bun.build({ entrypoints: ['website/.vitepress/theme/reader.worker.ts'], outdir: output, target: 'browser', format: 'esm', naming: 'reader.worker.js', define: { 'import.meta.env.BASE_URL': JSON.stringify('/') } })
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
  manifest.optionalDependencies = {}
  for (const platform of npmPlatforms) {
    const name = `${manifest.name}-${platform.key}`
    const directory = join(output, platform.key)
    await mkdir(join(directory, 'bin'), { recursive: true })
    const archive = `socli-${version}-${platform.target}`
    execFileSync('tar', ['-xzf', join(assets, `${archive}.tar.gz`), '-C', join(directory, 'bin'), '--strip-components=1', `${archive}/socli`])
    const binary = join(directory, 'bin/socli')
    await chmod(binary, 0o755)
    const metadata = {
      name, version, description: `socli native executable for ${platform.key}`,
      license: manifest.license, homepage: manifest.homepage, repository: manifest.repository,
      os: [platform.os], cpu: [platform.cpu], files: ['bin/socli', 'LICENSE', 'THIRD-PARTY-NOTICES.md'],
      publishConfig: { access: 'public' },
    }
    await writeFile(join(directory, 'package.json'), `${JSON.stringify(metadata, null, 2)}\n`)
    for (const file of ['LICENSE', 'THIRD-PARTY-NOTICES.md']) await copyFile(file, join(directory, file))
    platforms[platform.key] = { package: name, sha256: await digest(binary) }
    manifest.optionalDependencies[name] = version
  }
  const wrapper = join(output, 'cli')
  await mkdir(join(wrapper, 'bin'), { recursive: true })
  await writeFile(join(wrapper, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  await writeFile(join(wrapper, 'platforms.json'), `${JSON.stringify(platforms, null, 2)}\n`)
  await copyFile('packages/cli/bin/socli.js', join(wrapper, 'bin/socli.js'))
  await chmod(join(wrapper, 'bin/socli.js'), 0o755)
  await copyFile('packages/cli/README.md', join(wrapper, 'README.md'))
  for (const file of ['LICENSE', 'THIRD-PARTY-NOTICES.md']) await copyFile(file, join(wrapper, file))
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
  else if (command === 'checksums' && args.length === 1) await checksums(args[0])
  else throw new Error('usage: release.ts validate TAG | web WASM OUT | npm-native ASSETS OUT | formula VERSION REPOSITORY ASSETS | checksums ASSETS')
}
