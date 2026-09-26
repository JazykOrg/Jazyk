// Unpack a demo fixture (one JSON of path -> text, written by pack-demo.mjs) into a
// directory, so the native binary can work on it.
// See docs/frontends/wasm.md#keeping-the-demos-current.
//
//   node unpack-demo.mjs <fixture.json> <dir>
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'

const [src, dest] = process.argv.slice(2)
if (!src || !dest) {
  console.error('usage: node unpack-demo.mjs <fixture.json> <dir>')
  process.exit(1)
}
const files = JSON.parse(readFileSync(src, 'utf8'))
for (const [rel, text] of Object.entries(files)) {
  const p = join(dest, rel)
  mkdirSync(dirname(p), { recursive: true })
  writeFileSync(p, text)
}
console.log(`unpacked ${Object.keys(files).length} files into ${dest}`)
