// The links bar (docs/frontends/gui.md#the-board): at the foot of every window, what
// its item links to, grouped by kind with a count, the item's own kind first.
// Hovering a group shows its items as cards below the window (LinkCards, drawn by
// the board over everything); a click keeps them, a second click or Escape hides
// them, and a click on a card opens that item beside the window and focuses it.
import { memo, useEffect, useMemo, useState } from 'react'
import { useStore } from '@xyflow/react'
import { useDocs, useGraph, useView } from '../../lib/queries'
import { shorten } from '../../lib/present'
import { itemKind, itemTitle } from '../items'
import { KINDS } from '../kinds'
import { canonical, docOf, layoutFan, linkGroups, linkedDocs, nodeOf, type LinkGroup, type Rect } from './model'
import { useRuntime } from './runtime'
import { useDocText } from './nodes'

// A hover that leaves the bar hides the cards a beat later, so the pointer can
// travel from the bar down to them.
let hideTimer: number | null = null
function hideSoon() {
  if (hideTimer !== null) window.clearTimeout(hideTimer)
  hideTimer = window.setTimeout(() => {
    hideTimer = null
    const s = useRuntime.getState().shown
    if (s && !s.pinned) useRuntime.getState().hideGroup()
  }, 350)
}
function keep() {
  if (hideTimer !== null) window.clearTimeout(hideTimer)
  hideTimer = null
}

// What an item links to, grouped (model.ts linkGroups); a document's own links come
// from its text.
export function useLinkGroups(path: string): LinkGroup[] {
  const { data: g } = useGraph()
  const { data: docs } = useDocs()
  const doc = itemKind(path) === 'doc' ? docOf(path) : ''
  const text = useDocText(doc)
  return useMemo(() => {
    if (!g) return []
    const known = new Set((docs?.docs ?? []).map((d) => d.path))
    const linked = doc && text.data ? linkedDocs(doc, text.data.text, known) : []
    return linkGroups(g, path, linked)
  }, [g, docs, doc, text.data, path])
}

const WORD: Record<string, [string, string]> = {
  doc: ['document', 'documents'],
  entity: ['entity', 'entities'],
  requirement: ['requirement', 'requirements'],
  diagram: ['diagram', 'diagrams'],
}

const LinksBar = memo(function LinksBar({ path }: { path: string }) {
  const groups = useLinkGroups(path)
  const shown = useRuntime((s) => (s.shown?.path === path ? s.shown : null))
  if (groups.length === 0) return null
  return (
    <div className="bw-links nodrag" onMouseLeave={hideSoon} onMouseEnter={keep}>
      {groups.map((grp) => {
        const on = shown?.kind === grp.kind
        const n = grp.paths.length
        return (
          <button
            key={grp.kind}
            className={`bw-link-group k-${grp.kind}${on ? ' on' : ''}${on && shown?.pinned ? ' pinned' : ''}`}
            aria-expanded={on}
            onMouseEnter={() => {
              keep()
              const s = useRuntime.getState().shown
              if (!s?.pinned) useRuntime.getState().showGroup(path, grp.kind, false)
            }}
            onClick={(e) => {
              e.stopPropagation()
              const s = useRuntime.getState().shown
              if (s?.path === path && s.kind === grp.kind && s.pinned) useRuntime.getState().hideGroup()
              else useRuntime.getState().showGroup(path, grp.kind, true)
            }}
          >
            <span className="bw-link-glyph">{KINDS[grp.kind].glyph}</span> {n} {WORD[grp.kind][n === 1 ? 0 : 1]}
          </button>
        )
      })}
    </div>
  )
})

export default LinksBar

// ---- the cards a shown group lays out below its window ----

const CARD_W = 210
const CARD_H = 110
const INSET = 10

// A diagram's picture, scaled to fit its card.
function DiagramPreview({ id }: { id: string }) {
  const view = useView(id)
  const svg = view.data?.svg
  if (!svg) return <span className="bl-mini-empty">{view.isLoading ? 'drawing…' : 'no picture yet'}</span>
  return <span className="bl-mini-svg" aria-hidden="true" dangerouslySetInnerHTML={{ __html: svg }} />
}

// A card: the item's glyph and name in its kind's color; a diagram's card its
// picture under the name.
function MiniCard({ path, onOpen }: { path: string; onOpen: () => void }) {
  const { data: g } = useGraph()
  const kind = itemKind(path)
  let title = itemTitle(path)
  if (g && kind === 'entity') title = g.entities[canonical(g, nodeOf(path))]?.name ?? title
  else if (g && kind === 'requirement') title = g.requirements[canonical(g, nodeOf(path))]?.statement ?? title
  else if (g && kind === 'diagram') title = g.views[nodeOf(path)]?.title ?? title
  else if (kind === 'doc') title = docOf(path).split('/').pop() ?? title
  return (
    <button className={`bl-mini k-${kind}${kind === 'diagram' ? ' bl-mini-diagram' : ''}`} onClick={onOpen} title={kind === 'doc' ? docOf(path) : title} data-path={path}>
      <span className="bl-mini-title">
        <span className="bl-mini-glyph" aria-hidden="true">
          {KINDS[kind].glyph}
        </span>{' '}
        {shorten(title, 120)}
      </span>
      {kind === 'diagram' && <DiagramPreview id={nodeOf(path)} />}
    </button>
  )
}

// The shown group's cards under its window, with lines from the window's bottom
// center, as many as fit; a `+N` chip lists the rest.
export const LinkCards = memo(function LinkCards({ win, onOpen }: { win: Rect; onOpen: (path: string) => void }) {
  const shown = useRuntime((s) => s.shown)
  const groups = useLinkGroups(shown?.path ?? '')
  const paths = groups.find((x) => x.kind === shown?.kind)?.paths ?? []
  const [tx, ty, zoom] = useStore((s) => s.transform)
  const width = useStore((s) => s.width)
  const height = useStore((s) => s.height)
  const [listOpen, setListOpen] = useState(false)
  useEffect(() => setListOpen(false), [shown?.path, shown?.kind])
  useEffect(() => {
    if (!shown) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') useRuntime.getState().hideGroup()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [shown])
  if (!shown || paths.length === 0 || width < 50) return null
  const view: Rect = { x: INSET, y: INSET, w: width - 2 * INSET, h: height - 2 * INSET }
  const r: Rect = { x: win.x * zoom + tx, y: win.y * zoom + ty, w: win.w * zoom, h: win.h * zoom }
  const fan = layoutFan(
    r,
    paths.map(() => CARD_W),
    view,
    CARD_H,
  )
  const rest = paths.slice(paths.length - fan.more)
  const more = fan.slots.find((s) => s.index === -1)
  return (
    <div className="board-cards" onMouseEnter={keep} onMouseLeave={hideSoon}>
      {fan.anchor && (
        <svg className="br-lines" width={width} height={height} aria-hidden>
          {fan.slots.map((s) => {
            const a = fan.anchor!
            const cx = s.box.x + s.box.w / 2
            const mid = (a.y + s.box.y) / 2
            return <path key={s.index} d={`M ${a.x},${a.y} C ${a.x},${mid} ${cx},${mid} ${cx},${s.box.y}`} />
          })}
        </svg>
      )}
      {fan.slots.map((s) =>
        s.index < 0 ? (
          <button key="more" className={`bl-mini bl-more${listOpen ? ' on' : ''}`} style={{ left: s.box.x, top: s.box.y, width: s.box.w, height: s.box.h }} onClick={() => setListOpen((v) => !v)}>
            +{fan.more}
          </button>
        ) : (
          <div key={paths[s.index]} className="bl-mini-slot" style={{ left: s.box.x, top: s.box.y, width: s.box.w, height: s.box.h }}>
            <MiniCard path={paths[s.index]} onOpen={() => onOpen(paths[s.index])} />
          </div>
        ),
      )}
      {listOpen && more && (
        <ul className="bf-list" style={{ right: Math.max(INSET, width - more.box.x - more.box.w), bottom: height - more.box.y + 6 }}>
          {rest.map((p) => (
            <li key={p}>
              <button onClick={() => onOpen(p)} data-path={p}>
                {KINDS[itemKind(p)].glyph} <ListTitle path={p} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
})

function ListTitle({ path }: { path: string }) {
  const { data: g } = useGraph()
  const kind = itemKind(path)
  if (g && kind === 'entity') return <>{g.entities[canonical(g, nodeOf(path))]?.name ?? itemTitle(path)}</>
  if (g && kind === 'requirement') return <>{shorten(g.requirements[canonical(g, nodeOf(path))]?.statement ?? itemTitle(path), 80)}</>
  if (g && kind === 'diagram') return <>{g.views[nodeOf(path)]?.title ?? itemTitle(path)}</>
  return <>{kind === 'doc' ? docOf(path) : itemTitle(path)}</>
}
