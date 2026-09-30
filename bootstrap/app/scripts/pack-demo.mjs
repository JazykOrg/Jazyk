// Pack jazyk projects into the fixtures the browser backend boots from: every file
// of a project, path and content (text, or `{ base64 }` for a binary file), as one
// JSON the editor fetches when the project is picked. See docs/frontends/wasm.md#the-demo.
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
// the leases and worker registrations of a running server, and an archived out
// directory. A stale worker registration would be reaped on the first read in the
// tab, and that removal would count as an edit of the demo.
const SKIP = new Set(['trace', 'sessions', 'snapshots', 'leases', 'workers', 'node_modules', '__pycache__', '.git', '.jazyk.bak'])

// A text file is packed as its text; any other file (a built deck, an image) as
// `{ base64 }`, its bytes in standard base64, so a built artifact reaches the tab.
// A byte order mark is part of the file and is kept, so an unpacked file is the
// packed one byte for byte.
const utf8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true })
function textOf(bytes) {
  try {
    return utf8.decode(bytes)
  } catch {
    return null
  }
}

function walk(dir, files) {
  for (const name of readdirSync(dir)) {
    // Dotfiles are a machine's configuration (an editor's, an agent's), not the project.
    // The out directory is the one dotfolder a fixture carries.
    if (SKIP.has(name) || (name.startsWith('.') && name !== '.jazyk')) continue
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
  let count = 0
  let binary = 0
  for (const p of walk(src, []).sort()) {
    const rel = relative(src, p).split('\\').join('/')
    const raw = readFileSync(p)
    const text = textOf(raw)
    if (text === null) {
      out[rel] = { base64: raw.toString('base64') }
      binary++
    } else {
      out[rel] = text
    }
    bytes += raw.length
    count++
  }
  const file = join(dest, `${id}.json`)
  writeFileSync(file, JSON.stringify(out))
  const bin = binary > 0 ? `, ${binary} of them binary` : ''
  console.log(`packed ${count} files${bin}, ${(bytes / 1024).toFixed(0)}KB, into ${file}`)
}
