// The docs <-> deliverable join: a document links to the deliverable files that
// implement the requirements anchored in it, and back. Pure client-side join over
// the graph shards and the deliverable listing (docs/frontends/gui.md#files).
import { useMemo } from 'react'
import { useDeliverable, useGraph } from './queries'

export interface DocDelivLinks {
  docToFiles: Map<string, Set<string>>
  fileToDocs: Map<string, Set<string>>
  reqToFiles: Map<string, Set<string>>
  reqToDoc: Map<string, string>
}

const EMPTY: DocDelivLinks = {
  docToFiles: new Map(),
  fileToDocs: new Map(),
  reqToFiles: new Map(),
  reqToDoc: new Map(),
}

export function useDocDelivLinks(): DocDelivLinks {
  const graph = useGraph()
  const deliv = useDeliverable()
  return useMemo(() => {
    const g = graph.data
    const files = deliv.data?.files
    if (!g || !files) return EMPTY
    const reqToDoc = new Map<string, string>()
    for (const [rid, r] of Object.entries(g.requirements)) {
      // Only quote-provenanced requirements anchor in a document.
      if (r.source) reqToDoc.set(rid, r.source.doc)
    }
    const resolve = (id: string): string => {
      let cur = id
      const seen = new Set<string>()
      while (!reqToDoc.has(cur) && g.redirects[cur] && !seen.has(cur)) {
        seen.add(cur)
        cur = g.redirects[cur]
      }
      return cur
    }
    const docToFiles = new Map<string, Set<string>>()
    const fileToDocs = new Map<string, Set<string>>()
    const reqToFiles = new Map<string, Set<string>>()
    for (const f of files) {
      for (const raw of [...f.owners.requirements, ...f.owners.tests]) {
        const rid = resolve(raw)
        const doc = reqToDoc.get(rid)
        let set = reqToFiles.get(rid)
        if (!set) reqToFiles.set(rid, (set = new Set()))
        set.add(f.path)
        if (!doc) continue
        let df = docToFiles.get(doc)
        if (!df) docToFiles.set(doc, (df = new Set()))
        df.add(f.path)
        let fd = fileToDocs.get(f.path)
        if (!fd) fileToDocs.set(f.path, (fd = new Set()))
        fd.add(doc)
      }
    }
    return { docToFiles, fileToDocs, reqToFiles, reqToDoc }
  }, [graph.data, deliv.data])
}
