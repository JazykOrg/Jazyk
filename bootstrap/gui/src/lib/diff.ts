// Line diff for the editor gutters: which lines of the current text are new or
// changed against a baseline, and where baseline lines disappeared. Myers O(ND)
// over line hashes with prefix/suffix trimming; documents stay small.

export interface LineMarks {
  // 1-based current-side lines with no counterpart in the baseline.
  added: Set<number>
  // 1-based current-side lines that replaced deleted baseline lines.
  modified: Set<number>
  // 1-based current-side line -> baseline lines deleted just above it.
  deletedAbove: Map<number, number>
}

type Op = { t: 'same' | 'del' | 'add'; n: number }

// Myers greedy diff returning a run-length edit script (del before add per hunk).
function diffOps(a: string[], b: string[]): Op[] {
  // Trim the common prefix and suffix first; most edits are local.
  let pre = 0
  while (pre < a.length && pre < b.length && a[pre] === b[pre]) pre++
  let sufA = a.length
  let sufB = b.length
  while (sufA > pre && sufB > pre && a[sufA - 1] === b[sufB - 1]) {
    sufA--
    sufB--
  }
  const ca = a.slice(pre, sufA)
  const cb = b.slice(pre, sufB)
  const n = ca.length
  const m = cb.length
  const ops: Op[] = []
  if (pre > 0) ops.push({ t: 'same', n: pre })

  if (n === 0 || m === 0) {
    if (n > 0) ops.push({ t: 'del', n })
    if (m > 0) ops.push({ t: 'add', n: m })
  } else {
    const max = n + m
    const offset = max
    let v = new Int32Array(2 * max + 1)
    const trace: Int32Array[] = []
    let found = false
    for (let d = 0; d <= max && !found; d++) {
      trace.push(v.slice())
      const next = v.slice()
      for (let k = -d; k <= d; k += 2) {
        let x: number
        if (k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])) x = v[offset + k + 1]
        else x = v[offset + k - 1] + 1
        let y = x - k
        while (x < n && y < m && ca[x] === cb[y]) {
          x++
          y++
        }
        next[offset + k] = x
        if (x >= n && y >= m) found = true
      }
      v = next
    }
    // Walk the trace back into (del|add|same) steps, then reverse.
    const rev: Op[] = []
    let x = n
    let y = m
    for (let d = trace.length - 1; d > 0; d--) {
      const pv = trace[d]
      const k = x - y
      let pk: number
      if (k === -d || (k !== d && pv[offset + k - 1] < pv[offset + k + 1])) pk = k + 1
      else pk = k - 1
      const px = pv[offset + pk]
      const py = px - pk
      while (x > px && y > py) {
        rev.push({ t: 'same', n: 1 })
        x--
        y--
      }
      if (x === px) {
        rev.push({ t: 'add', n: 1 })
        y--
      } else {
        rev.push({ t: 'del', n: 1 })
        x--
      }
    }
    while (x > 0 && y > 0) {
      rev.push({ t: 'same', n: 1 })
      x--
      y--
    }
    while (x > 0) {
      rev.push({ t: 'del', n: 1 })
      x--
    }
    while (y > 0) {
      rev.push({ t: 'add', n: 1 })
      y--
    }
    // Forward order, then normalize each change hunk to del-run followed by
    // add-run (the backtrack may interleave them).
    let dels = 0
    let adds = 0
    const flush = () => {
      if (dels > 0) ops.push({ t: 'del', n: dels })
      if (adds > 0) ops.push({ t: 'add', n: adds })
      dels = 0
      adds = 0
    }
    for (let i = rev.length - 1; i >= 0; i--) {
      const op = rev[i]
      if (op.t === 'same') {
        flush()
        const last = ops[ops.length - 1]
        if (last && last.t === 'same') last.n += op.n
        else ops.push({ ...op })
      } else if (op.t === 'del') dels += op.n
      else adds += op.n
    }
    flush()
  }
  const sufLen = a.length - sufA
  if (sufLen > 0) {
    const last = ops[ops.length - 1]
    if (last && last.t === 'same') last.n += sufLen
    else ops.push({ t: 'same', n: sufLen })
  }
  return ops
}

// Classify the current side: paired del+add lines are modifications, the surplus of
// an add run is additions, a bare del run anchors a deletion marker on the line
// that follows it.
export function lineMarks(baseline: string, current: string): LineMarks {
  const a = baseline.split('\n')
  const b = current.split('\n')
  const marks: LineMarks = { added: new Set(), modified: new Set(), deletedAbove: new Map() }
  let bLine = 1
  let pendingDel = 0
  for (const op of diffOps(a, b)) {
    if (op.t === 'same') {
      if (pendingDel > 0) {
        marks.deletedAbove.set(bLine, (marks.deletedAbove.get(bLine) ?? 0) + pendingDel)
        pendingDel = 0
      }
      bLine += op.n
    } else if (op.t === 'del') {
      pendingDel += op.n
    } else {
      const paired = Math.min(pendingDel, op.n)
      for (let i = 0; i < op.n; i++) {
        if (i < paired) marks.modified.add(bLine + i)
        else marks.added.add(bLine + i)
      }
      pendingDel = Math.max(0, pendingDel - paired)
      if (pendingDel > 0) {
        marks.deletedAbove.set(bLine + op.n, (marks.deletedAbove.get(bLine + op.n) ?? 0) + pendingDel)
        pendingDel = 0
      }
      bLine += op.n
    }
  }
  if (pendingDel > 0) marks.deletedAbove.set(bLine, (marks.deletedAbove.get(bLine) ?? 0) + pendingDel)
  return marks
}
