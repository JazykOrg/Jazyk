// The controls every item shares (docs/frontends/gui.md#workspace): the close of a
// file or the pin of anything else on its title bar, and the switch between Tabs and
// the Board at the end of its second bar.
import { createContext, useContext } from 'react'
import { isPinned, itemKind, itemTitle, useWorkspace } from './items'
import { useNodeName } from '../lib/present'
import { KINDS } from './kinds'

// A thumbtack, filled when pinned.
function PinIcon({ pinned }: { pinned: boolean }) {
  return (
    <svg className="pin-icon" width="13" height="13" viewBox="0 0 24 24" aria-hidden="true" fill={pinned ? 'currentColor' : 'none'} stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="12" y1="17" x2="12" y2="22" />
      <path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24Z" />
    </svg>
  )
}

// An item's close: in Tabs on every tab, on the Board on every window.
export function CloseButton({ title, onClose, className }: { title: string; onClose: () => void; className: string }) {
  return (
    <button
      className={className}
      aria-label={`close ${title}`}
      title="Close"
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation()
        onClose()
      }}
    >
      <span aria-hidden="true">×</span>
    </button>
  )
}

// A board window's pin: a file opens pinned, anything else unpinned; an unpinned
// window stays open only while the focus holds it (docs/frontends/gui.md#workspace).
export function PinButton({ path, title, className }: { path: string; title: string; className: string }) {
  const pinned = useWorkspace((w) => {
    const it = w.items.find((i) => i.path === path)
    return it ? isPinned(it) : false
  })
  return (
    <button
      className={`${className} pin${pinned ? ' on' : ''}`}
      aria-label={pinned ? `unpin ${title}` : `pin ${title}`}
      aria-pressed={pinned}
      title={pinned ? 'Pinned: stays open. Unpin to let it close when nothing holds it.' : 'Pin to keep it open; unpinned, it closes once the focus no longer holds it'}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation()
        useWorkspace.getState().togglePin(path)
      }}
    >
      <PinIcon pinned={pinned} />
    </button>
  )
}

// The switch, for the item a frame shows: the frame (a tab, a board window) provides
// it, and the item's second bar (the editor's bar, or the frame's own for any other
// page) draws it.
export interface Arrange {
  path: string
  to: 'tabs' | 'board'
}

export const ArrangeContext = createContext<Arrange | null>(null)

export function ArrangeButton() {
  const a = useContext(ArrangeContext)
  if (!a) return null
  const toBoard = a.to === 'board'
  return (
    <button
      className="ws-arrange-btn nodrag"
      aria-label={toBoard ? 'Collapse into the Board' : 'Expand into Tabs'}
      title={toBoard ? 'Collapse into the Board' : 'Expand into Tabs'}
      onClick={(e) => {
        e.stopPropagation()
        useWorkspace.getState().setArrangement(a.to, a.path)
      }}
    >
      {toBoard ? '⤡' : '⤢'}
    </button>
  )
}

// The kinds whose page draws its own second bar (the editor's, the product file
// viewer's, the generated page viewer's), which carries the switch itself.
export function hasOwnBar(path: string): boolean {
  const k = itemKind(path)
  return k === 'doc' || k === 'file' || k === 'test' || k === 'generated'
}

// The second bar of a page without one of its own: its kind and its name (never an
// id in the default mode), and the switch at its end.
export function ItemBar({ path }: { path: string }) {
  const p = path.split(/[?#]/)[0]
  const kind = itemKind(p)
  const name = useNodeName()
  const segs = p.split('/').filter(Boolean)
  const node = kind === 'entity' || kind === 'requirement' || kind === 'diagram' ? decodeURIComponent(segs[segs.length - 1] ?? '') : ''
  const label = node ? name(node) : kind === 'other' || kind === 'product' ? itemTitle(p) : ''
  return (
    <div className="ws-bar2">
      <span className="ws-bar2-path">
        <span className="ws-bar2-kind">{KINDS[kind].label}</span>
        {label && <span> · {label.length > 90 ? `${label.slice(0, 89)}…` : label}</span>}
      </span>
      <ArrangeButton />
    </div>
  )
}
