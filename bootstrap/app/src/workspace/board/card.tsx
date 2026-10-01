// The linkage card (docs/frontends/gui.md#the-linkage): a head (a verb, a portal's or
// a reference's title), two lines of the requirements behind the link, and a line
// that expands the card to all of them. A requirement's line reveals its quote in its
// document; the button at its right opens the requirement. What the caret rests on
// is marked: its requirements come first and are highlighted.
import type { ReactNode } from 'react'
import { useGraph } from '../../lib/queries'
import { useRuntime } from './runtime'

// A folded card is narrow, each statement cut to one line; an expanded one is wider
// and shows every statement whole.
export const CARD_W = 200
export const CARD_W_OPEN = 340
export const HEAD_H = 24
export const LINE_H = 20
const PAD = 6
// The lines a folded card shows.
export const FOLDED = 2
const OPEN_MAX_H = 380

// A card's size for its statements, folded or expanded.
export function cardBox(texts: string[], open: boolean): { w: number; h: number } {
  const n = texts.length
  if (n === 0) return { w: CARD_W, h: HEAD_H }
  if (!open) return { w: CARD_W, h: HEAD_H + Math.min(n, FOLDED) * LINE_H + (n > FOLDED ? LINE_H : 0) + PAD }
  const perLine = Math.floor((CARD_W_OPEN - 44) / 6.3)
  const lines = texts.reduce((a, t) => a + Math.max(1, Math.ceil(t.length / perLine)), 0)
  return { w: CARD_W_OPEN, h: Math.min(OPEN_MAX_H, HEAD_H + lines * 16 + n * 6 + LINE_H + PAD) }
}

// The requirements in a card's order: the caret's first.
export function cardOrder(reqs: string[], hot: Set<string>): string[] {
  return [...reqs.filter((r) => hot.has(r)), ...reqs.filter((r) => !hot.has(r))]
}

export default function LinkCard({
  id,
  head,
  reqs,
  hotReqs,
  hotHead,
  onReveal,
  onOpenRequirement,
}: {
  // The card's identity for the one expanded card.
  id: string
  head: ReactNode
  reqs: string[]
  hotReqs: Set<string>
  hotHead?: boolean
  onReveal: (rid: string) => void
  onOpenRequirement: (rid: string) => void
}) {
  const { data: g } = useGraph()
  const open = useRuntime((s) => s.expanded === id)
  const toggle = useRuntime((s) => s.toggleLink)
  const ordered = cardOrder(reqs, hotReqs)
  const shown = open ? ordered : ordered.slice(0, FOLDED)
  return (
    <>
      <div className={`lc-head${hotHead ? ' hot' : ''}`}>{head}</div>
      {shown.length > 0 && (
        <ul className={`lc-reqs${open ? ' open' : ''}`}>
          {shown.map((rid) => {
            const text = g?.requirements[rid]?.statement ?? rid
            return (
              <li key={rid} className={hotReqs.has(rid) ? 'hot' : ''} data-req={rid}>
                <button className="lc-req" onClick={() => onReveal(rid)} title={`${text}\n(click: show it in its document)`}>
                  {text}
                </button>
                <button className="lc-open" onClick={() => onOpenRequirement(rid)} title="Open the requirement as a window" aria-label="Open the requirement">
                  ↗
                </button>
              </li>
            )
          })}
        </ul>
      )}
      {reqs.length > FOLDED && (
        <button className="lc-more" onClick={() => toggle(id)} aria-expanded={open}>
          {open ? 'fewer ▴' : `${reqs.length - FOLDED} more ▾`}
        </button>
      )}
    </>
  )
}
