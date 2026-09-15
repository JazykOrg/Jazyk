// What the editor draws over a document from the graph itself, no language
// server needed (docs/frontends/gui.md#editor): the requirement markers colored by
// rollup, and the question and proposal bands under their sentences.
import { useMemo } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { post, type Diagnostic, type Graph, type VerifyRow } from '../lib/api'
import { useApp } from '../lib/store'
import { words } from '../lib/words'
import type { InlineBand, ReqMark } from './CmHost'

// The rollup class of one requirement: its trace status, or a question standing
// on it. The margin marker takes the color.
export function markClass(status: string | undefined, asked: boolean): string {
  if (asked) return 'req-asked'
  if (!status || status === 'missing' || status === 'unverified' || status === 'unimplemented') return 'req-extracted'
  if (status === 'verified') return 'req-verified'
  if (status === 'failing' || status === 'partial') return 'req-failing'
  if (status.startsWith('stale')) return 'req-stale'
  return 'req-extracted'
}

function statusWords(status: string | undefined): string {
  if (!status || status === 'missing') return 'extracted, not yet built'
  return status
}

const PROPOSAL_RULES = new Set(['ratification-pending', 'invented-choice'])

// Every open, unsuppressed, unanswered prompted diagnostic that anchors in the
// document: on a section reference, or on a requirement whose quote is here.
export function bandsFor(graph: Graph, doc: string): { id: string; d: Diagnostic; quote?: string; line?: number }[] {
  const out: { id: string; d: Diagnostic; quote?: string; line?: number }[] = []
  for (const [id, d] of Object.entries(graph.diagnostics)) {
    if ((d.lifecycle ?? 'open') !== 'open' || d.triage === 'suppressed' || !d.prompt) continue
    if (d.answer && d.answer.status !== 'failed') continue
    let quote: string | undefined
    let line: number | undefined
    for (const s of d.subjects ?? []) {
      const r = graph.requirements[s]
      if (r?.source?.doc === doc) {
        quote = r.source.quote
        break
      }
      if (s === doc) line = 1
      const m = s.match(/^(.+\.md)#(.*)$/)
      if (m && m[1] === doc) line = 1
    }
    if (quote === undefined && line === undefined) continue
    out.push({ id, d, quote, line })
  }
  return out
}

export function useReqMarks(graph: Graph | undefined, rows: Record<string, VerifyRow>, doc: string): ReqMark[] {
  return useMemo(() => {
    if (!graph) return []
    const asked = new Set<string>()
    for (const b of bandsFor(graph, doc)) for (const s of b.d.subjects ?? []) asked.add(s)
    const marks: ReqMark[] = []
    for (const [rid, r] of Object.entries(graph.requirements)) {
      if (r.source?.doc !== doc) continue
      const status = rows[rid]?.status
      marks.push({
        quote: r.source.quote,
        rid,
        cls: markClass(status, asked.has(rid)),
        tip: `${rid}: ${asked.has(rid) ? 'a question is open on this sentence' : statusWords(status)}\n${r.statement}`,
      })
    }
    return marks
  }, [graph, rows, doc])
}

function el(tag: string, cls?: string, text?: string): HTMLElement {
  const e = document.createElement(tag)
  if (cls) e.className = cls
  if (text !== undefined) e.textContent = text
  return e
}

export function useInlineBands(graph: Graph | undefined, doc: string): InlineBand[] {
  const qc = useQueryClient()
  const dev = useApp((a) => a.dev)
  return useMemo(() => {
    if (!graph) return []
    const w = words(dev)
    const refresh = () => {
      void qc.invalidateQueries({ queryKey: ['graph'] })
      void qc.invalidateQueries({ queryKey: ['inbox'] })
    }
    return bandsFor(graph, doc).map(({ id, d, quote, line }) => ({
      key: `${id}:${d.answer?.status ?? ''}`,
      quote,
      line,
      render: () => {
        const proposal = PROPOSAL_RULES.has(d.rule)
        const root = el('div', `jz-band-body sev-${d.severity}`)
        const head = el('div', 'jz-band-head')
        head.appendChild(el('span', 'jz-band-kind', proposal ? w.proposal : w.question))
        head.appendChild(el('span', 'jz-band-rule mono', d.rule))
        root.appendChild(head)
        root.appendChild(el('div', 'jz-band-msg', d.message))
        if (d.prompt?.question) root.appendChild(el('div', 'jz-band-q', d.prompt.question))
        const opts = el('div', 'jz-band-opts')
        const busy = (on: boolean) => {
          for (const b of Array.from(opts.querySelectorAll('button, input'))) (b as HTMLButtonElement).disabled = on
        }
        const answer = async (body: Record<string, unknown>) => {
          busy(true)
          try {
            await post(`/api/questions/${encodeURIComponent(id)}/answer`, body)
            root.replaceChildren(el('div', 'jz-band-msg muted', 'answered, being handled…'))
          } catch (e) {
            busy(false)
            root.appendChild(el('div', 'jz-band-msg v-bad', (e as Error).message))
          } finally {
            refresh()
          }
        }
        ;(d.prompt?.options ?? []).forEach((o, i) => {
          const b = el('button', o.edit ? 'edit' : '', `${o.edit ? (proposal ? 'Accept: ' : '✎ ') : ''}${o.label}`)
          b.title = o.edit ? 'applies the edit to the document and resolves' : 'recorded and handled by the agent'
          b.onclick = () => void answer({ option: i })
          opts.appendChild(b)
        })
        if (d.prompt?.freeform) {
          const input = el('input', 'jz-band-free') as HTMLInputElement
          input.type = 'text'
          input.placeholder = 'answer in your own words, enter to send'
          input.onkeydown = (e) => {
            if (e.key === 'Enter' && input.value.trim()) void answer({ text: input.value.trim() })
          }
          opts.appendChild(input)
        }
        root.appendChild(opts)
        return root
      },
    }))
  }, [graph, doc, qc, dev])
}
