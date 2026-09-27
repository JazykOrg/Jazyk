// @vitest-environment node
// The pack/unpack round trip of a demo fixture (docs/frontends/wasm.md#the-demo):
// what pack-demo.mjs leaves out is exactly the skipped entries and the binary files,
// and every file that is packed unpacks byte for byte.
import { describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const scripts = dirname(fileURLToPath(import.meta.url))

function run(script, args) {
  const r = spawnSync(process.execPath, [join(scripts, script), ...args], { encoding: 'utf8' })
  expect(r.status, r.stderr).toBe(0)
  return r.stdout
}

function put(root, rel, content) {
  const p = join(root, rel)
  mkdirSync(dirname(p), { recursive: true })
  writeFileSync(p, content)
}

function tree(root) {
  const out = new Map()
  const walk = (dir) => {
    for (const name of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, name.name)
      if (name.isDirectory()) walk(p)
      else out.set(relative(root, p).split('\\').join('/'), readFileSync(p))
    }
  }
  walk(root)
  return out
}

describe('pack-demo and unpack-demo', () => {
  it('round-trips a project minus the skipped entries and the binary files', () => {
    const work = mkdtempSync(join(tmpdir(), 'jazyk-pack-'))
    try {
      const src = join(work, 'project')
      const kept = {
        'jazyk.toml': 'docs = ["docs/*.md"]\n',
        'docs/main.md': '# Shop\n\nThe Cart holds items.\n',
        'docs/nested/deep.md': 'Ünïcödé and a tab\t and CRLF\r\n',
        '.jazyk/status.yaml': 'version: 2\ngeneration: 1\n',
        '.jazyk/graph/entities.yaml': 'ent:cart:\n  name: Cart\n',
        '.jazyk/diagrams/class/shop.puml': '@startuml\nclass Cart\n@enduml\n',
        'code/empty.txt': '',
        // A byte order mark is part of the file's bytes.
        'docs/bom.md': Buffer.from([0xef, 0xbb, 0xbf, ...Buffer.from('# With a BOM\n')]),
      }
      const skipped = {
        '.editorconfig': 'root = true\n',
        'docs/.hidden.md': 'a dotfile\n',
        '.jazyk/trace/g1.jsonl': '{"event":"note"}\n',
        '.jazyk/sessions/s1.md': 'transcript\n',
        '.jazyk.bak/status.yaml': 'version: 1\n',
        'code/__pycache__/main.cpython-310.pyc': 'cached\n',
        'code/node_modules/dep/index.js': 'module.exports = 1\n',
        // Not UTF-8: a lone continuation byte and an overlong sequence.
        'deck/built.bin': Buffer.from([0x00, 0xff, 0xfe, 0x80, 0xc0, 0xaf, 0x41]),
      }
      for (const [rel, content] of Object.entries(kept)) put(src, rel, content)
      for (const [rel, content] of Object.entries(skipped)) put(src, rel, content)

      const demos = join(work, 'demos')
      const packed = run('pack-demo.mjs', [demos, `demo=${src}`])
      expect(packed).toContain(`packed ${Object.keys(kept).length} files`)
      expect(packed).toContain('1 binary file(s) left out')

      const fixture = JSON.parse(readFileSync(join(demos, 'demo.json'), 'utf8'))
      expect(Object.keys(fixture).sort()).toEqual(Object.keys(kept).sort())

      const dest = join(work, 'unpacked')
      run('unpack-demo.mjs', [join(demos, 'demo.json'), dest])
      const got = tree(dest)
      expect([...got.keys()].sort()).toEqual(Object.keys(kept).sort())
      for (const [rel, content] of Object.entries(kept)) {
        expect(got.get(rel).equals(Buffer.from(content)), rel).toBe(true)
      }
    } finally {
      rmSync(work, { recursive: true, force: true })
    }
  })
})
