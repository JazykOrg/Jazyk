// A demo fixture as the page reads it (docs/frontends/wasm.md#the-demo): each path
// maps to its text, or to `{ base64 }` for a file that is not UTF-8 text (a built
// deck, an image). The tab's tree is seeded with the text as is and with a binary
// file's bytes decoded.

// Standard base64 to bytes. `atob` is in every browser and in node, so the page and
// the unit tests decode the same way.
export function fromBase64(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

// Bytes to standard base64, in chunks so a large file never overflows the call stack.
export function toBase64(bytes: Uint8Array): string {
  let bin = ''
  const CHUNK = 0x8000
  for (let i = 0; i < bytes.length; i += CHUNK) bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK))
  return btoa(bin)
}

// A fixture's files, ready to seed: text stays text, `{ base64 }` becomes bytes. A
// value of any other shape is refused, so a broken fixture fails where it is read.
export function decodeFixture(raw: Record<string, unknown>): Record<string, string | Uint8Array> {
  const out: Record<string, string | Uint8Array> = {}
  for (const [path, v] of Object.entries(raw)) {
    if (typeof v === 'string') out[path] = v
    else if (v && typeof v === 'object' && typeof (v as { base64?: unknown }).base64 === 'string') out[path] = fromBase64((v as { base64: string }).base64)
    else throw new Error(`the fixture's ${path} is neither text nor { base64 }`)
  }
  return out
}
