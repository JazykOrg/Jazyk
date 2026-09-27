// @vitest-environment node
// ledger-count.py counts a generation ledger's rows and how many pass, for
// refresh-demos.sh and check-demos.sh (docs/frontends/wasm.md#keeping-the-demos-current).
import { describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const script = join(dirname(fileURLToPath(import.meta.url)), 'ledger-count.py')

// Four rows in the ledger's shape (docs/consumers/gen.md): a row whose verdict is a
// quoted scalar, a row with a nested criterion verdict beside its own, a failing row,
// and a row never verified.
const ledger = `medium:
  form: JavaScript CLI utility
entities: {}
requirements:
  req:main-js-1:
    entity: ent:sort-cli
    files:
    - main.js
    verdict: 'pass'
    evidence: 'node --test: pass 1 | fail 0'
  req:main-js-2:
    entity: ent:sort-cli
    tests:
      crit:main-js-2:
        criterion_hash: 5ca9f474303bd4b5
        verdict: pass
        evidence: |
          the run said
          verdict: fail (this is evidence text, not a verdict)
    verdict: pass
  req:main-js-3:
    entity: ent:sort-cli
    tests:
      crit:main-js-3:
        verdict: fail
    verdict: fail
    evidence: 'exit 1'
  req:main-js-4:
    entity: ent:sort-cli
    files:
    - main.js
`

function count(text) {
  const work = mkdtempSync(join(tmpdir(), 'jazyk-ledger-'))
  try {
    const p = join(work, 'ledger.yaml')
    writeFileSync(p, text)
    const r = spawnSync('python3', [script, p], { encoding: 'utf8' })
    expect(r.status, r.stderr).toBe(0)
    return r.stdout.trim()
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}

describe('ledger-count.py', () => {
  it('prints the passing rows and the rows', () => {
    // Rows 1 and 2 pass; row 3 fails; row 4 has no verdict, so it does not pass.
    expect(count(ledger)).toBe('2 4')
  })

  it('counts nothing for an empty ledger', () => {
    expect(count('medium:\n  form: x\nrequirements: {}\n')).toBe('0 0')
  })
})
