import { spawnSync } from 'node:child_process'
import { mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createHash } from 'node:crypto'
import { expect, test } from 'bun:test'

const script = join(import.meta.dir, '../website/public/install.sh')

function run(env: Record<string, string>, args: string[] = []) {
  return spawnSync('sh', [script, ...args], {
    env: { PATH: process.env.PATH ?? '', ...env },
    encoding: 'utf8',
  })
}

function printed(env: Record<string, string>) {
  const child = run({
    SOCLI_INSTALL_PRINT: '1',
    SOCLI_KERNEL: 'Linux',
    SOCLI_VERSION: '0.1.2',
    ...env,
  })
  expect(child.status).toBe(0)
  expect(child.stderr).toBe('')
  return Object.fromEntries(child.stdout.trim().split('\n').map((line) => line.split('=')))
}

test('debian and ubuntu install the deb', () => {
  expect(printed({ SOCLI_ID: 'debian', SOCLI_ARCH: 'x86_64' })).toEqual({
    version: '0.1.2',
    family: 'deb',
    asset: 'socli_0.1.2-1_amd64.deb',
  })
  expect(printed({ SOCLI_ID: 'linuxmint', SOCLI_ID_LIKE: 'ubuntu debian', SOCLI_ARCH: 'aarch64' }).asset).toBe(
    'socli_0.1.2-1_arm64.deb',
  )
  expect(printed({ SOCLI_ID: 'raspbian', SOCLI_ID_LIKE: 'debian', SOCLI_ARCH: 'armv7l' }).asset).toBe(
    'socli_0.1.2-1_armhf.deb',
  )
})

test('rpm and alpine install their packages', () => {
  expect(printed({ SOCLI_ID: 'fedora', SOCLI_ARCH: 'x86_64' }).asset).toBe('socli-0.1.2-1.x86_64.rpm')
  expect(printed({ SOCLI_ID: 'opensuse-tumbleweed', SOCLI_ID_LIKE: 'opensuse suse', SOCLI_ARCH: 'aarch64' }).asset).toBe(
    'socli-0.1.2-1.aarch64.rpm',
  )
  expect(printed({ SOCLI_ID: 'alpine', SOCLI_ARCH: 'x86_64' })).toMatchObject({
    family: 'apk',
    asset: 'socli_0.1.2-r1_x86_64.apk',
  })
  expect(printed({ SOCLI_ID: 'alpine', SOCLI_ARCH: 'i686' }).asset).toBe('socli_0.1.2-r1_x86.apk')
})

test('arch and omarchy install the musl binary', () => {
  expect(printed({ SOCLI_ID: 'arch', SOCLI_ARCH: 'x86_64' })).toEqual({
    version: '0.1.2',
    family: 'tar',
    asset: 'socli-0.1.2-x86_64-unknown-linux-musl.tar.gz',
  })
  expect(printed({ SOCLI_ID: 'omarchy', SOCLI_ID_LIKE: 'arch', SOCLI_ARCH: 'x86_64' }).family).toBe('tar')
  expect(printed({ SOCLI_ID: 'omarchy', SOCLI_ID_LIKE: 'arch', SOCLI_ARCH: 'aarch64' }).asset).toBe(
    'socli-0.1.2-aarch64-unknown-linux-musl.tar.gz',
  )
  expect(printed({ SOCLI_ID: 'arch', SOCLI_ARCH: 'i686' }).asset).toBe('socli-0.1.2-i686-unknown-linux-musl.tar.gz')
})

test('a version prefix and an unknown machine are refused', () => {
  const bad = run({ SOCLI_INSTALL_PRINT: '1', SOCLI_KERNEL: 'Linux', SOCLI_VERSION: '../etc', SOCLI_ID: 'debian', SOCLI_ARCH: 'x86_64' })
  expect(bad.status).not.toBe(0)
  expect(bad.stderr).toContain('refusing version')
  const arm = run({ SOCLI_INSTALL_PRINT: '1', SOCLI_KERNEL: 'Linux', SOCLI_VERSION: '0.1.2', SOCLI_ID: 'debian', SOCLI_ARCH: 'armv6l' })
  expect(arm.status).not.toBe(0)
  expect(arm.stderr).toContain('unsupported architecture')
})

test('macOS is sent to Homebrew', () => {
  const child = run({ SOCLI_KERNEL: 'Darwin' })
  expect(child.status).not.toBe(0)
  expect(child.stderr).toContain('brew install socli')
})

test('the checksum accepts one matching line and rejects a different file', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'socli-install-'))
  const file = join(directory, 'socli_0.1.2-1_amd64.deb')
  await writeFile(file, 'package-bytes')
  const hash = createHash('sha256').update('package-bytes').digest('hex')
  const sums = join(directory, 'SHA256SUMS')
  await writeFile(sums, `${hash}  socli_0.1.2-1_amd64.deb\n`)
  const good = run({ SOCLI_INSTALL_CHECK: '1' }, [file, sums])
  expect(good.status).toBe(0)
  expect(good.stdout.trim()).toBe('ok')
  await writeFile(file, 'other-bytes')
  const bad = run({ SOCLI_INSTALL_CHECK: '1' }, [file, sums])
  expect(bad.status).not.toBe(0)
  expect(bad.stderr).toContain('checksum did not match')
})
