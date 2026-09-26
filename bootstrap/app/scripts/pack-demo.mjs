// Pack jazyk projects into the fixtures the browser backend boots from: every file
// of a project, path and text, as one JSON the editor fetches when the project is
// picked. See docs/frontends/wasm.md#the-demo.
//
//   node pack-demo.mjs <out-dir> <id>=<project-dir> [<id>=<project-dir> ...]
import { readFileSync, writeFileSync, readdirSync, lstatSync, mkdirSync } from 'node:fs'
import { join, relative } from 'node:path'

const [dest, ...pairs] = process.argv.slice(2)
if (!dest || pairs.length === 0) {
  console.error('usage: node pack-demo.mjs <out-dir> <id>=<project-dir> ...')
  process.exit(1)
}

// Everything under the project except what a tab has no use for: the traces of the
// builds that produced it, the session transcripts, the snapshots kept for replays,
// the leases of a running server, and an archived out directory.
const SKIP = new Set(['trace', 'sessions', 'snapshots', 'leases', 'node_modules', '.git', 'jazyk-out.bak'])

function walk(dir, files) {
  for (const name of readdirSync(dir)) {
    // Dotfiles are a machine's configuration (an editor's, an agent's), not the project.
    if (SKIP.has(name) || name.startsWith('.')) continue
    const p = join(dir, name)
    const st = lstatSync(p)
    if (st.isDirectory()) walk(p, files)
    // A socket a server left behind is not a file of the project.
    else if (st.isFile()) files.push(p)
  }
  return files
}

mkdirSync(dest, { recursive: true })
for (const pair of pairs) {
  const eq = pair.indexOf('=')
  const id = pair.slice(0, eq)
  const src = pair.slice(eq + 1)
  const out = {}
  let bytes = 0
  const files = walk(src, []).sort()
  for (const p of files) {
    const rel = relative(src, p).split('\\').join('/')
    const text = readFileSync(p, 'utf8')
    out[rel] = text
    bytes += text.length
  }
  const file = join(dest, `${id}.json`)
  writeFileSync(file, JSON.stringify(out))
  console.log(`packed ${files.length} files, ${(bytes / 1024).toFixed(0)}KB, into ${file}`)
}
