// Pack a jazyk project into the fixture the browser backend boots from: every file
// of the project, path and text, as one JSON the demo imports.
// See docs/frontends/wasm.md#the-demo.
import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync } from 'node:fs'
import { join, relative, dirname } from 'node:path'

const src = process.argv[2]
const dest = process.argv[3]
if (!src || !dest) {
  console.error('usage: node pack-demo.mjs <project-dir> <out.json>')
  process.exit(1)
}

// Everything under the project except what a tab has no use for: the traces of the
// builds that produced it, and the session transcripts.
const SKIP = new Set(['trace', 'sessions', 'leases', 'node_modules', '.git'])

const files = []
function walk(dir) {
  for (const name of readdirSync(dir)) {
    if (SKIP.has(name)) continue
    const p = join(dir, name)
    const st = statSync(p)
    if (st.isDirectory()) walk(p)
    else files.push(p)
  }
}
walk(src)

const out = {}
let bytes = 0
for (const p of files.sort()) {
  const rel = relative(src, p).split('\\').join('/')
  const text = readFileSync(p, 'utf8')
  out[rel] = text
  bytes += text.length
}

mkdirSync(dirname(dest), { recursive: true })
writeFileSync(dest, JSON.stringify(out))
console.log(`packed ${files.length} files, ${(bytes / 1024).toFixed(0)}KB, into ${dest}`)
