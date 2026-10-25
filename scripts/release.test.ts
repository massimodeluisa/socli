import { describe, expect, test } from 'bun:test'
import { chmod, mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createHash } from 'node:crypto'
import { homebrewFormula, nativeNpmPackages, npmPlatforms, releaseNotes, releaseVersion, validateReleaseOrder, validateVersions } from './release'
import { selectPlatform, resolveBinary } from '../packages/cli/bin/socli.js'
import { npmPublishPlan, publicationState } from './publish-npm'
import { execFileSync } from 'node:child_process'

describe('release tags', () => {
  test('stable and prerelease channels have explicit version syntax', () => {
    expect(releaseVersion('v1.2.3')).toEqual({ version: '1.2.3', prerelease: false })
    for (const channel of ['alpha', 'beta', 'rc']) {
      expect(releaseVersion(`v1.2.3-${channel}.2`).prerelease).toBe(true)
    }
    for (const tag of ['main', '1.2.3', 'v1', 'v01.2.3', 'v1.2.3-dev', 'v1.2.3-rc.01', 'v1.2.3+build', 'v1.2.3\n']) {
      expect(() => releaseVersion(tag)).toThrow()
    }
  })
  test('an old stable tag cannot downgrade the release channels', () => {
    const releases = [{ tagName: 'v1.10.0', isDraft: false, isPrerelease: false }]
    expect(() => validateReleaseOrder('1.9.0', releases)).toThrow()
    expect(() => validateReleaseOrder('1.10.0', releases)).toThrow()
    expect(() => validateReleaseOrder('1.11.0', releases)).not.toThrow()
    expect(() => validateReleaseOrder('2.0.0-rc.1', releases)).not.toThrow()
    expect(() => validateReleaseOrder('1.0.0', [{ ...releases[0], isDraft: true }])).not.toThrow()
  })
})

describe('release metadata', () => {
  test('extracts exactly one dated release rather than unreleased notes', () => {
    const changelog = '# Changelog\n\n## [Unreleased]\n\n- Future work.\n\n## [1.2.3] - 2026-10-24\n\n### Fixed\n\n- Fixed the reader.\n\n## [1.2.2] - 2026-10-20\n\n- Older change.\n'
    expect(releaseNotes(changelog, '1.2.3')).toBe('### Fixed\n\n- Fixed the reader.\n')
    expect(() => releaseNotes(changelog, '1.2.4')).toThrow()
    expect(() => releaseNotes('## [1.2.3] - 2026-02-30\n\n- Fixed it.\n', '1.2.3')).toThrow()
    expect(() => releaseNotes('## [1.2.3] - 2026-10-24\n', '1.2.3')).toThrow()
  })
  test('checks root package versions, ignoring dependency versions', () => {
    const cargo = '[package]\nname = "socli"\nversion = "1.2.3"\n'
    const lock = 'version = 4\n[[package]]\nname = "dep"\nversion = "9.0.0"\n[[package]]\nname = "socli"\nversion = "1.2.3"\n'
    expect(() => validateVersions('1.2.3', cargo, lock, '{"version":"1.2.3"}')).not.toThrow()
    expect(() => validateVersions('1.2.4', cargo, lock, '{"version":"1.2.3"}')).toThrow()
    expect(() => validateVersions('1.2.3', cargo, lock, '{"version":"1.2.2"}')).toThrow()
    expect(() => validateVersions('1.2.3', cargo, lock.replace('version = "1.2.3"', 'version = "1.2.2"'), '{"version":"1.2.3"}')).toThrow()
    expect(() => validateVersions('1.2.3', cargo, lock, '{"version":"1.2.3"}', '{"version":"1.2.2"}')).toThrow()
  })
})

test('Homebrew downloads the release archives and verifies their actual hashes', async () => {
  const assets = await mkdtemp(join(tmpdir(), 'socli-formula-'))
  for (const target of ['aarch64-apple-darwin', 'x86_64-apple-darwin', 'aarch64-unknown-linux-musl', 'x86_64-unknown-linux-musl']) {
    await writeFile(join(assets, `socli-1.2.3-${target}.tar.gz`), target)
  }
  const formula = await homebrewFormula('1.2.3', 'owner/project', assets)
  expect(formula.match(/sha256 /g)?.length).toBe(4)
  expect(formula).toContain('https://github.com/owner/project/releases/download/v1.2.3/socli-1.2.3-x86_64-apple-darwin.tar.gz')
  expect(formula).toContain(createHash('sha256').update('aarch64-unknown-linux-musl').digest('hex'))
})

test('web distribution contains a valid wasm module and installable npm metadata', async () => {
  const output = await mkdtemp(join(tmpdir(), 'socli-web-package-'))
  const wasm = join(output, 'empty.wasm')
  await writeFile(wasm, Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]))
  const child = Bun.spawn(['bun', 'scripts/release.ts', 'web', wasm, join(output, 'npm')], {
    env: { ...process.env, NPM_PACKAGE_NAME: '@test/socli-web', GITHUB_REPOSITORY: 'owner/project' },
    stdout: 'pipe', stderr: 'pipe',
  })
  expect(await new Response(child.stderr).text()).toBe('')
  expect(await child.exited).toBe(0)
  const manifest = JSON.parse(await readFile(join(output, 'npm/package.json'), 'utf8'))
  expect(manifest.name).toBe('@test/socli-web')
  expect(manifest.repository.url).toBe('git+https://github.com/owner/project.git')
  expect(WebAssembly.validate(await readFile(join(output, 'npm/socli.wasm')))).toBe(true)
  expect(manifest.exports['.'].types).toBe('./index.d.ts')
  expect(await readFile(join(output, 'npm/index.js'), 'utf8')).toContain('mountReader')
  expect(await readFile(join(output, 'npm/reader.worker.js'), 'utf8')).toContain('socli_theme')
  expect(await readFile(join(output, 'npm/reader.css'), 'utf8')).toContain('.wasm-row')
  expect((await readFile(join(output, 'npm/fonts/GeistMono-Regular.otf'))).byteLength).toBeGreaterThan(0)
  expect(await readFile(join(output, 'npm/fonts/Apache-2.0.txt'), 'utf8')).toContain('Apache License')
})

for (const exists of [false, true]) {
  test(`Homebrew ${exists ? 'updates with the existing SHA' : 'creates the first formula'} through the Contents API`, async () => {
    const fixture = await mkdtemp(join(tmpdir(), 'socli-tap-'))
    await mkdir(join(fixture, 'artifacts'))
    await writeFile(join(fixture, 'artifacts/socli.rb'), 'class Socli < Formula\nend\n')
    await writeFile(join(fixture, 'gh'), `#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == *"--method PUT"* ]]; then
  cat > "$PAYLOAD_FILE"
elif [[ "$2" == repos/owner/tap ]]; then
  echo main
elif [[ "$2" == */git/trees/main ]]; then
  echo '${exists ? '["Formula"]' : '[]'}'
elif [[ "$2" == *"/contents/Formula?ref=main" ]]; then
  echo '["socli.rb"]'
elif [[ "$2" == *"/contents/Formula/socli.rb?ref=main" ]]; then
  echo existing-file-sha
else
  exit 1
fi
`)
    await chmod(join(fixture, 'gh'), 0o755)
    const child = Bun.spawn(['bash', join(import.meta.dir, 'update-homebrew.sh')], {
      cwd: fixture,
      env: { ...process.env, PATH: `${fixture}:${process.env.PATH}`, HOMEBREW_TAP: 'owner/tap', VERSION: '1.2.3', PAYLOAD_FILE: join(fixture, 'payload.json') },
      stdout: 'pipe', stderr: 'pipe',
    })
    expect(await new Response(child.stderr).text()).toBe('')
    expect(await child.exited).toBe(0)
    const payload = JSON.parse(await readFile(join(fixture, 'payload.json'), 'utf8'))
    expect(payload.branch).toBe('main')
    expect(payload.sha).toBe(exists ? 'existing-file-sha' : undefined)
    expect(Buffer.from(payload.content, 'base64').toString()).toBe('class Socli < Formula\nend\n')
  })
}

describe('native npm launcher', () => {
  test('selects all supported platforms and rejects unsupported Windows and older ARM', () => {
    for (const platform of npmPlatforms) expect(selectPlatform(platform.os, platform.cpu, 7)).toBe(platform.key)
    expect(() => selectPlatform('win32', 'x64')).toThrow('Windows')
    expect(() => selectPlatform('linux', 'arm', 6)).toThrow('ARMv7')
    expect(() => selectPlatform('linux', 'arm')).toThrow('ARMv7')
  })
  test('requires the correct installed binary and verifies its hash', async () => {
    const fixture = await mkdtemp(join(tmpdir(), 'socli-npm-binary-'))
    const binary = join(fixture, 'socli')
    await writeFile(binary, 'native executable fixture')
    const platforms = { 'linux-x64': { package: '@test/socli-linux-x64', sha256: createHash('sha256').update('native executable fixture').digest('hex') } }
    expect(resolveBinary(platforms, 'linux', 'x64', undefined, () => binary)).toBe(binary)
    expect(() => resolveBinary(platforms, 'linux', 'x64', undefined, () => { throw new Error('missing') })).toThrow('optional dependencies')
    await writeFile(binary, 'different executable')
    expect(() => resolveBinary(platforms, 'linux', 'x64', undefined, () => binary)).toThrow('checksum')
  })
})

test('native npm packages pin platform versions and execute through the npm bin symlink', async () => {
  const fixture = await mkdtemp(join(tmpdir(), 'socli-npm-release-'))
  const assets = join(fixture, 'assets')
  await mkdir(assets)
  const version = JSON.parse(await readFile('packages/cli/package.json', 'utf8')).version
  for (const platform of npmPlatforms) {
    const name = `socli-${version}-${platform.target}`
    await mkdir(join(fixture, name))
    await writeFile(join(fixture, name, 'socli'), '#!/usr/bin/env node\nconsole.log(JSON.stringify(process.argv.slice(2)));\nprocess.exit(Number(process.env.SOCLI_TEST_EXIT ?? 0));\n')
    execFileSync('tar', ['-czf', join(assets, `${name}.tar.gz`), '-C', fixture, name])
  }
  const output = join(fixture, 'npm')
  await nativeNpmPackages(assets, output)
  const wrapper = JSON.parse(await readFile(join(output, 'cli/package.json'), 'utf8'))
  expect(Object.keys(wrapper.optionalDependencies)).toHaveLength(6)
  for (const platform of npmPlatforms) {
    const manifest = JSON.parse(await readFile(join(output, platform.key, 'package.json'), 'utf8'))
    expect(wrapper.optionalDependencies[manifest.name]).toBe(version)
    expect(manifest.os).toEqual([platform.os])
    expect(manifest.cpu).toEqual([platform.cpu])
    const entry = JSON.parse(await readFile(join(output, 'cli/platforms.json'), 'utf8'))[platform.key]
    expect(entry.sha256).toBe(createHash('sha256').update(await readFile(join(output, platform.key, 'bin/socli'))).digest('hex'))
  }
  for (const platform of [...npmPlatforms.map((item) => item.key), 'cli']) {
    execFileSync('npm', ['pack', join(output, platform), '--pack-destination', assets, '--json'], { stdio: 'pipe' })
  }
  const plan = await npmPublishPlan(assets)
  expect(plan.at(-1)?.manifest.name).toBe(wrapper.name)
  expect(plan.map((item) => item.manifest.name)).toHaveLength(7)
  const current = npmPlatforms.find((item) => item.os === process.platform && item.cpu === process.arch)
  if (!current) throw new Error('test host has no native npm package')
  const manifest = JSON.parse(await readFile(join(output, current.key, 'package.json'), 'utf8'))
  const binaryTarball = plan.find((item) => item.manifest.name === manifest.name)?.path
  const installed = join(fixture, 'installed')
  execFileSync('npm', ['install', '--offline', '--ignore-scripts', '--no-audit', '--no-fund', '--prefix', installed, '--cache', join(fixture, 'cache'), plan.at(-1).path, binaryTarball], { stdio: 'pipe' })
  const child = Bun.spawn(['node', join(installed, 'node_modules/.bin/socli'), '--example', 'a b', '$(literal)'], {
    env: { ...process.env, SOCLI_TEST_EXIT: '7' }, stdout: 'pipe', stderr: 'pipe',
  })
  expect(await new Response(child.stderr).text()).toBe('')
  expect(JSON.parse(await new Response(child.stdout).text())).toEqual(['--example', 'a b', '$(literal)'])
  expect(await child.exited).toBe(7)
}, 30000)

test('npm publication retries only skip versions with identical bytes', () => {
  expect(publicationState({ status: 0, stdout: '"sha512-same"', stderr: '' }, 'sha512-same')).toBe('published')
  expect(() => publicationState({ status: 0, stdout: '"sha512-other"', stderr: '' }, 'sha512-same')).toThrow('different package bytes')
  expect(publicationState({ status: 1, stdout: '{"error":{"code":"E404"}}', stderr: '' }, 'sha512-same')).toBe('missing')
  expect(() => publicationState({ status: 1, stdout: '{"error":{"code":"E401"}}', stderr: 'unauthorized' }, 'sha512-same')).toThrow('unauthorized')
})
