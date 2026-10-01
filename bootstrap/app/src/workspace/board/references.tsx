// References (docs/frontends/gui.md#the-board): while the caret in the focused
// document rests on something the graph knows, the unopened entities it concerns
// show below the window, transient, each a linkage card (card.tsx) titled by the
// entity. Lines leave the window's bottom center and fan down to the row; a click on
// a title opens the entity beside the document and focuses it. Drawn in screen
// space over the board, like the portals.
import { memo, useMemo } from 'react'
import { useStore } from '@xyflow/react'
import { layoutFan, type Rect } from './model'
import { useRuntime } from './runtime'
import LinkCard, { cardBox } from './card'
import { useGraph } from '../../lib/queries'

const INSET = 10

export interface Reference {
  entity: string
  name: string
  kind: string
  // The requirements between the document and the entity.
  reqs: string[]
}

export const refCardId = (entity: string) => `ref:${entity}`

export const References = memo(function References({
  win,
  refs,
  hotEntity,
  hotReqs,
  onOpen,
  onReveal,
  onOpenRequirement,
}: {
  // The focused window's rect on the board.
  win: Rect
  refs: Reference[]
  hotEntity: string | null
  hotReqs: Set<string>
  onOpen: (entity: string) => void
  onReveal: (rid: string) => void
  onOpenRequirement: (rid: string) => void
}) {
  const [tx, ty, zoom] = useStore((s) => s.transform)
  const width = useStore((s) => s.width)
  const height = useStore((s) => s.height)
  const expanded = useRuntime((s) => s.expanded)
  const { data: g } = useGraph()

  const fan = useMemo(() => {
    if (width < 50 || height < 50 || refs.length === 0) return null
    const view: Rect = { x: INSET, y: INSET, w: width - 2 * INSET, h: height - 2 * INSET }
    const r: Rect = { x: win.x * zoom + tx, y: win.y * zoom + ty, w: win.w * zoom, h: win.h * zoom }
    if (r.x > view.x + view.w || r.x + r.w < view.x || r.y > view.y + view.h || r.y + r.h < view.y) return null
    const boxes = refs.map((x) => cardBox(x.reqs.map((id) => g?.requirements[id]?.statement ?? id), expanded === refCardId(x.entity)))
    return layoutFan(
      r,
      boxes.map((b) => b.w),
      view,
      Math.max(...boxes.map((b) => b.h)),
    )
  }, [win, refs, tx, ty, zoom, width, height, expanded, g])

  if (!fan) return null
  return (
    <div className="board-refs" data-testid="references">
      {fan.anchor && (
        <svg className="br-lines" width={width} height={height} aria-hidden>
          {fan.slots
            .filter((s) => s.index >= 0)
            .map((s) => {
              const a = fan.anchor!
              const cx = s.box.x + s.box.w / 2
              const mid = (a.y + s.box.y) / 2
              return <path key={s.index} d={`M ${a.x},${a.y} C ${a.x},${mid} ${cx},${mid} ${cx},${s.box.y}`} />
            })}
        </svg>
      )}
      {fan.slots
        .filter((s) => s.index >= 0)
        .map((s) => {
          const x = refs[s.index]
          return (
            <div key={x.entity} className="br-card lc" style={{ left: s.box.x, top: s.box.y, width: s.box.w, maxHeight: s.box.h }} data-entity={x.entity}>
              <LinkCard
                id={refCardId(x.entity)}
                hotHead={x.entity === hotEntity}
                head={
                  <button className="br-title" onClick={() => onOpen(x.entity)} title={`Open ${x.name}${x.kind ? ` (${x.kind})` : ''} on the board`}>
                    <span className="bl-glyph">◆</span>
                    <span className="bl-name">{x.name}</span>
                    {x.kind && <span className="br-kind">{x.kind}</span>}
                  </button>
                }
                reqs={x.reqs}
                hotReqs={hotReqs}
                onReveal={onReveal}
                onOpenRequirement={onOpenRequirement}
              />
            </div>
          )
        })}
    </div>
  )
})
