// The linkage card (docs/frontends/gui.md#the-linkage): a head (a verb, a portal's or
// a reference's title), two lines of the requirements behind the link, and a line
// that expands the card to all of them. A requirement's line reveals its quote in its
// document; the button at its right opens the requirement. What the caret rests on
// is marked: its requirements come first and are highlighted.
import type { ReactNode } from 'react'
import { useGraph } from '../../lib/queries'
import { useRuntime } from './runtime'

export const CARD_W = 280
export const HEAD_H = 24
export const LINE_H = 20
const PAD = 6
// The lines a folded card shows.
export const FOLDED = 2
// The lines an expanded card shows before it scrolls.
const MOST = 12

// A card's height for its requirement count, folded or expanded.
export function cardHeight(n: number, open: boolean): number {
  if (n === 0) return HEAD_H
  if (open) return HEAD_H + Math.min(n, MOST) * LINE_H + LINE_H + PAD
  return HEAD_H + Math.min(n, FOLDED) * LINE_H + (n > FOLDED ? LINE_H : 0) + PAD
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
