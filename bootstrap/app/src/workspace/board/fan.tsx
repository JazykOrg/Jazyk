// The fan (docs/frontends/gui.md#the-board): the focused window's entities that are
// not open yet, transient, in a row below it with lines from its bottom center. A
// click on a chip opens the entity beside the window for good; a `+N` chip lists the
// rest. Drawn in screen space over the board, like the portals.
import { memo, useMemo, useState } from 'react'
import { useStore } from '@xyflow/react'
import { fanChipWidth, layoutFan, type Rect } from './model'

const INSET = 10

export interface FanEntity {
  id: string
  name: string
  kind: string
}

export const Fan = memo(function Fan({
  win,
  entities,
  hot,
  onOpen,
}: {
  // The focused window's rect on the board.
  win: Rect
  // The unopened entities it brings, in the fan's order.
  entities: FanEntity[]
  // The entity under the caret.
  hot: string | null
  onOpen: (id: string) => void
}) {
  const [tx, ty, zoom] = useStore((s) => s.transform)
  const width = useStore((s) => s.width)
  const height = useStore((s) => s.height)
  const [listOpen, setListOpen] = useState(false)

  const fan = useMemo(() => {
    if (width < 50 || height < 50 || entities.length === 0) return null
    const view: Rect = { x: INSET, y: INSET, w: width - 2 * INSET, h: height - 2 * INSET }
    const r: Rect = { x: win.x * zoom + tx, y: win.y * zoom + ty, w: win.w * zoom, h: win.h * zoom }
    // A window out of view brings no fan.
    if (r.x > view.x + view.w || r.x + r.w < view.x || r.y > view.y + view.h || r.y + r.h < view.y) return null
    return layoutFan(r, entities.map((e) => fanChipWidth(e.name)), view)
  }, [win, entities, tx, ty, zoom, width, height])

  if (!fan) return null
  const rest = entities.slice(entities.length - fan.more)
  const more = fan.slots.find((s) => s.index === -1)
  return (
    <div className="board-fan" data-testid="fan">
      {fan.anchor && (
        <svg className="bf-lines" width={width} height={height} aria-hidden>
          {fan.slots.map((s) => {
            const a = fan.anchor!
            const cx = s.box.x + s.box.w / 2
            const mid = (a.y + s.box.y) / 2
            return <path key={s.index} className={s.index >= 0 && entities[s.index].id === hot ? 'hot' : ''} d={`M ${a.x},${a.y} C ${a.x},${mid} ${cx},${mid} ${cx},${s.box.y}`} />
          })}
        </svg>
      )}
      {fan.slots.map((s) => {
        if (s.index < 0) {
          return (
            <button key="more" className={`bf-chip bf-more${listOpen ? ' on' : ''}`} style={{ left: s.box.x, top: s.box.y, width: s.box.w, height: s.box.h }} onClick={() => setListOpen((v) => !v)} aria-expanded={listOpen} title={`${fan.more} more`}>
              +{fan.more}
            </button>
          )
        }
        const e = entities[s.index]
        return (
          <button
            key={e.id}
            className={`bf-chip${e.id === hot ? ' hot' : ''}`}
            style={{ left: s.box.x, top: s.box.y, width: s.box.w, height: s.box.h }}
            onClick={() => onOpen(e.id)}
            title={`Open ${e.name}${e.kind ? ` (${e.kind})` : ''} on the board`}
            data-entity={e.id}
          >
            <span className="bf-glyph">◆</span>
            <span className="bf-name">{e.name}</span>
          </button>
        )
      })}
      {listOpen && more && (
        <ul className="bf-list" style={{ right: Math.max(INSET, width - more.box.x - more.box.w), bottom: height - more.box.y + 6 }}>
          {rest.map((e) => (
            <li key={e.id}>
              <button
                onClick={() => {
                  setListOpen(false)
                  onOpen(e.id)
                }}
                data-entity={e.id}
              >
                <span className="bf-glyph">◆</span> {e.name}
                {e.kind && <span className="bf-kind"> {e.kind}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
})
