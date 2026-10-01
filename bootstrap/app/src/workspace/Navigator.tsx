// The workspace's one navigator (docs/frontends/gui.md#workspace): the focused
// item's area sections when it has them, the whole project directory as a tree with
// every file typed by kind, and under it the open document's Outline, each part
// scrolling on its own. A single click opens a preview item, a double click keeps it.
import { useLocation } from 'react-router'
import { AreaSections, hasAreaSections } from '../shell/Navigator'
import { ProjectExplorer } from '../workbench/Explorer'
import Outline from '../shell/Outline'
import { pinPath } from './actions'
import { hrefPath } from './paths'

// The next navigation is a navigator single click: it opens a preview item. The flag
// lapses on its own, so a click that navigates nowhere does not mark a later one.
let previewUntil = 0
export function takePreviewOpen(): boolean {
  const on = Date.now() < previewUntil
  previewUntil = 0
  return on
}

function anchorPath(target: EventTarget | null): string | null {
  const a = (target as HTMLElement | null)?.closest?.('a')
  return a ? hrefPath(a.getAttribute('href')) : null
}

function AreaFold() {
  const loc = useLocation()
  if (!hasAreaSections(loc.pathname)) return null
  return (
    <div className="wsn-area" data-fold="area">
      <AreaSections path={loc.pathname} />
    </div>
  )
}

export default function WorkspaceNavigator() {
  return (
    <aside
      className="ws-nav wsn"
      aria-label="Navigator"
      // A single click on a row opens a preview; a double click keeps it.
      onClickCapture={(e) => {
        if (e.detail <= 1 && anchorPath(e.target)) previewUntil = Date.now() + 600
      }}
      onDoubleClickCapture={(e) => {
        const p = anchorPath(e.target)
        if (p) pinPath(p)
      }}
    >
      <AreaFold />
      <div className="wsn-tree">
        <ProjectExplorer />
      </div>
      <div className="wsn-outline">
        <Outline />
      </div>
    </aside>
  )
}
