import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const targetRoot = process.env.CARGO_TARGET_DIR
  ? resolve(process.env.CARGO_TARGET_DIR)
  : resolve(root, 'target')
const binary = [join(targetRoot, 'release', 'lycodl'), join(targetRoot, 'release', 'lycodl.exe')].find(
  (path) => existsSync(path),
)

if (!binary) {
  console.error('smoke: build the release binary first: cargo build --release')
  process.exit(1)
}

function run(args) {
  return execFileSync(binary, args, {
    encoding: 'utf8',
    timeout: 30_000,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
}

const version = run(['--version']).trim()
if (!version.startsWith('lycodl ')) throw new Error(`unexpected --version output: ${version}`)

const help = run(['--help'])
if (help.length < 100) throw new Error('--help output is unexpectedly small')

const schema = JSON.parse(run(['--schema']))
if (!Array.isArray(schema) || schema.length < 5) {
  throw new Error(`--schema returned ${Array.isArray(schema) ? schema.length : 'non-array'} commands`)
}
if (!schema.every((command) => typeof command.name === 'string' && command.schema)) {
  throw new Error('--schema contains malformed commands')
}

console.log(`smoke: ${version.split('\n')[0]}; ${schema.length} commands; --help/--schema OK`)
