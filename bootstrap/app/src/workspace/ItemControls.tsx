// The controls every item shares (docs/frontends/gui.md#workspace): the close of a
// file or the pin of anything else on its title bar, and the switch between Tabs and
// the Board at the end of its second bar.
import { createContext, useContext } from 'react'
import { isFileItem, useWorkspace } from './items'

function PinIcon({ pinned }: { pinned: boolean }) {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
      <path
        d="M10.5 1.5l4 4-2 1-2.5 2.5.5 3.5-1.5 1.5-3-3-4 4-1-1 4-4-3-3 1.5-1.5 3.5.5L9.5 3.5z"
        fill={pinned ? 'currentColor' : 'none'}
        stroke="currentColor"
        strokeWidth="1.2"
        strokeLinejoin="round"
      />
    </svg>
  )
}

// A file's close, or the pin of a transient item.
export function CloseOrPin({ path, title, onClose, className }: { path: string; title: string; onClose: () => void; className: string }) {
  const pinned = useWorkspace((w) => !!w.items.find((i) => i.path === path)?.pinned)
  if (isFileItem(path)) {
    return (
      <button
        className={className}
        aria-label={`close ${title}`}
        onMouseDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation()
          onClose()
        }}
      >
        ×
      </button>
    )
  }
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

// The second bar of any page but a document (whose editor draws its own): the page's
// path, and the switch at its end.
export function ItemBar({ path }: { path: string }) {
  return (
    <div className="ws-bar2">
      <span className="mono ws-bar2-path">{path.split(/[?#]/)[0]}</span>
      <ArrangeButton />
    </div>
  )
}
