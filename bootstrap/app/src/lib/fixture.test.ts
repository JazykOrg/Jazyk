import { describe, expect, it } from 'vitest'
import { decodeFixture, fromBase64, toBase64 } from './fixture'

describe('decodeFixture', () => {
  it('keeps text and decodes { base64 } into the same bytes', () => {
    const bytes = Uint8Array.from({ length: 70_000 }, (_, i) => (i * 13) % 256)
    const files = decodeFixture({ 'docs/a.md': '# A\n', 'dist/deck.pptx': { base64: toBase64(bytes) } })
    expect(files['docs/a.md']).toBe('# A\n')
    expect(files['dist/deck.pptx']).toBeInstanceOf(Uint8Array)
    expect(Array.from(files['dist/deck.pptx'] as Uint8Array)).toEqual(Array.from(bytes))
  })

  it('decodes what node encodes, and encodes the same', () => {
    const b64 = Buffer.from([0, 255, 254, 128, 65]).toString('base64')
    expect(Array.from(fromBase64(b64))).toEqual([0, 255, 254, 128, 65])
    expect(toBase64(new Uint8Array([0, 255, 254, 128, 65]))).toBe(b64)
  })

  it('refuses a value of any other shape', () => {
    expect(() => decodeFixture({ 'x.bin': 42 })).toThrow(/x\.bin/)
    expect(() => decodeFixture({ 'x.bin': { bytes: 'AA==' } })).toThrow(/x\.bin/)
  })
})
