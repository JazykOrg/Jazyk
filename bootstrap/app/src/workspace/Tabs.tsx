// The Tabs arrangement (docs/frontends/gui.md#workspace): editor groups
// with tabs that split right or down, from a tab's menu or by dragging a tab to an
// edge, over the workspace's open items. Each tab renders its item's page against
// its own path. The layout (groups, splits, sizes) is remembered per project beside
// the items; a hidden tab stays mounted only while it is among the few most recently
// shown.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { create } from 'zustand'
import {
  DockviewReact,
  type DockviewApi,
  type DockviewGroupPanel,
  type DockviewReadyEvent,
  type GetTabContextMenuItemsParams,
  type IDockviewPanel,
  type IDockviewPanelHeaderProps,
  type IDockviewPanelProps,
  type SerializedDockview,
  themeLight,
} from 'dockview-react'
import 'dockview-react/dist/styles/dockview.css'
import AppRoutes from '../routes/AppRoutes'
import { useNodeName } from '../lib/present'
import { itemKind, itemTitle, useWorkspace, type Item } from './items'
import { itemKey, touchRecent } from './paths'
import { KINDS } from './kinds'
import { ArrangeContext, CloseButton, hasOwnBar, ItemBar } from './ItemControls'
import { closePath, pinPath } from './actions'

// At most this many tabs stay mounted; the rest render when shown again.
export const MOUNTED = 8

const useRecent = create<{ keys: string[]; touch: (k: string) => void }>((set) => ({
  keys: [],
  touch: (k) => set((s) => (s.keys[0] === k ? s : { keys: touchRecent(s.keys, k, MOUNTED) })),
}))

const layoutKey = (project: string) => `jazyk.workspace.layout:${project}`

function readLayout(project: string | null): SerializedDockview | null {
  if (!project) return null
  try {
    const raw = localStorage.getItem(layoutKey(project))
    return raw ? (JSON.parse(raw) as SerializedDockview) : null
  } catch {
    return null
  }
}

function writeLayout(project: string | null, api: DockviewApi) {
  if (!project) return
  try {
    localStorage.setItem(layoutKey(project), JSON.stringify(api.toJSON()))
  } catch {
    // not remembered
  }
}


// The title a tab shows: a model node by its name, a page by its last segment.
export function useItemTitle(path: string): string {
  const name = useNodeName()
  const kind = itemKind(path)
  if (kind === 'entity' || kind === 'requirement' || kind === 'diagram') {
    const seg = itemKey(path).split('/').pop() ?? ''
    const id = decodeURIComponent(seg)
    const n = name(id)
    if (n && n !== id) return n.length > 40 ? `${n.slice(0, 39)}…` : n
  }
  return itemTitle(itemKey(path))
}

function ItemTab({ api, params }: IDockviewPanelHeaderProps<{ path: string }>) {
  const item = useWorkspace((w) => w.items.find((i) => itemKey(i.path) === itemKey(params.path)))
  const path = item?.path ?? params.path
  const title = useItemTitle(path)
  const kind = itemKind(path)
  return (
    <div
      className={`wst-tab kp k-${kind}${item?.preview ? ' preview' : ''}`}
      data-path={itemKey(path)}
      title={itemKey(path)}
      onDoubleClick={() => pinPath(path)}
      onMouseDown={(e) => {
        // The middle button closes, as in an editor.
        if (e.button === 1) {
          e.preventDefault()
          api.close()
        }
      }}
    >
      <span className={`wst-icon k-${kind}`} aria-hidden="true">
        {KINDS[kind].glyph}
      </span>
      <span className="wst-title">{title}</span>
      <CloseButton className="wst-close" title={title} onClose={() => api.close()} />
    </div>
  )
}

function ItemPanel({ api, params }: IDockviewPanelProps<{ path: string }>) {
  const key = itemKey(params.path)
  const item = useWorkspace((w) => w.items.find((i) => itemKey(i.path) === key))
  const recent = useRecent((r) => r.keys.includes(key))
  const [visible, setVisible] = useState(api.isVisible)
  useEffect(() => {
    const d = api.onDidVisibilityChange((e) => setVisible(e.isVisible))
    return () => d.dispose()
  }, [api])
  const path = item?.path ?? params.path
  const arrange = useMemo(() => ({ path, to: 'board' as const }), [path])
  return (
    <div className={`wst-item k-${itemKind(path)}`} data-item={key}>
      <ArrangeContext.Provider value={arrange}>
        {/* A document's second bar is its editor's; any other page gets this one. */}
        {!hasOwnBar(path) && <ItemBar path={path} />}
        {visible || recent ? <AppRoutes location={path} /> : null}
      </ArrangeContext.Provider>
    </div>
  )
}

function Watermark() {
  return (
    <div className="ws-scroll">
      <div className="empty-state" style={{ marginTop: 60 }}>
        <b>Nothing open</b>
        Open a document, a product file, or a diagram from the navigator. A single click previews it; a double click keeps it.
      </div>
    </div>
  )
}

const components = { item: ItemPanel }
const tabComponents = { item: ItemTab }
const theme = { ...themeLight, name: 'jazyk', className: 'dockview-theme-light' }

function split(api: DockviewApi, panel: IDockviewPanel, group: DockviewGroupPanel, direction: 'right' | 'below') {
  const g = api.addGroup({ referenceGroup: group, direction })
  panel.api.moveTo({ group: g })
  panel.api.setActive()
}

export default function Tabs() {
  const project = useWorkspace((w) => w.project)
  const items = useWorkspace((w) => w.items)
  const focused = useWorkspace((w) => w.focused)
  const apiRef = useRef<DockviewApi | null>(null)
  // Set while the store's items are applied to the layout: the layout's own events
  // then are echoes, not a person's moves.
  const syncing = useRef(false)
  const [ready, setReady] = useState(0)
  // Until the store's items are first applied, the layout's activations are its own
  // restore (the tab last in front when it was saved), not a person's choice: the
  // focused item wins, as when the Board expands into Tabs.
  const applied = useRef(false)

  // The store's items onto the layout: add the missing, drop the closed, retarget the
  // moved, and bring the focused to the front. A preview replaced in the store takes
  // the replaced tab's place.
  const apply = useCallback((list: Item[], front: string | null) => {
    const api = apiRef.current
    if (!api) return
    syncing.current = true
    try {
      const want = new Map(list.map((i) => [itemKey(i.path), i]))
      const gone = api.panels.filter((p) => !want.has(p.id))
      for (const [key, item] of want) {
        const panel = api.getPanel(key)
        if (panel) {
          if ((panel.params as { path?: string } | undefined)?.path !== item.path) panel.api.updateParameters({ path: item.path })
          continue
        }
        const replaced = gone.find((p) => p.group && p.group.panels.includes(p))
        const at = replaced ? replaced.group.panels.indexOf(replaced) : -1
        // A new tab joins the active group, else the first: added inactive, the tabs
        // would otherwise each start a group of their own (a split).
        const group = replaced?.group ?? api.activeGroup ?? api.groups[0]
        api.addPanel({
          id: key,
          component: 'item',
          tabComponent: 'item',
          params: { path: item.path },
          title: itemTitle(key),
          renderer: 'always',
          inactive: true,
          ...(group ? { position: { referenceGroup: group, index: at >= 0 ? at : undefined } } : {}),
        })
      }
      for (const p of gone) if (api.getPanel(p.id)) api.removePanel(p)
      const fk = front ? itemKey(front) : null
      const fp = fk ? api.getPanel(fk) : undefined
      if (fp && api.activePanel !== fp) fp.api.setActive()
      if (fk) useRecent.getState().touch(fk)
    } finally {
      syncing.current = false
    }
    if (!applied.current) {
      // The restore's own activation may land after this; the focused item stays.
      window.setTimeout(() => {
        applied.current = true
        const fk = useWorkspace.getState().focused
        const fp = fk ? api.getPanel(itemKey(fk)) : undefined
        if (fp && api.activePanel !== fp) {
          syncing.current = true
          try {
            fp.api.setActive()
          } finally {
            syncing.current = false
          }
        }
      }, 0)
    }
    writeLayout(useWorkspace.getState().project, api)
  }, [])

  const onReady = useCallback(
    (e: DockviewReadyEvent) => {
      const api = e.api
      apiRef.current = api
      const saved = readLayout(useWorkspace.getState().project)
      if (saved) {
        syncing.current = true
        try {
          api.fromJSON(saved)
        } catch {
          api.clear()
        } finally {
          syncing.current = false
        }
      }
      // A person's moves: the tab they bring to the front becomes the focused item, a
      // tab they close leaves the store. A move between groups fires no removal (the
      // layout holds its events while it moves a panel), so a removal is a close.
      api.onDidActivePanelChange((ev) => {
        if (syncing.current || !applied.current || !ev.panel) return
        const path = (ev.panel.params as { path?: string } | undefined)?.path
        if (!path) return
        useRecent.getState().touch(ev.panel.id)
        const ws = useWorkspace.getState()
        const cur = ws.items.find((i) => itemKey(i.path) === ev.panel!.id)
        if (cur && ws.focused !== cur.path) ws.focusItem(cur.path)
      })
      api.onDidRemovePanel((p) => {
        if (syncing.current || api.getPanel(p.id)) return
        closePath(p.id)
      })
      api.onDidLayoutChange(() => {
        if (!syncing.current) writeLayout(useWorkspace.getState().project, api)
      })
      setReady((n) => n + 1)
    },
    [],
  )

  // A different project brings its own layout.
  const lastProject = useRef(project)
  useEffect(() => {
    const api = apiRef.current
    if (!api || lastProject.current === project) return
    lastProject.current = project
    syncing.current = true
    try {
      const saved = readLayout(project)
      if (saved) api.fromJSON(saved)
      else api.clear()
    } catch {
      api.clear()
    } finally {
      syncing.current = false
    }
  }, [project])

  useEffect(() => {
    if (ready) apply(items, focused)
  }, [ready, items, focused, apply])

  const menu = useCallback((p: GetTabContextMenuItemsParams) => {
    const single = p.group.panels.length < 2
    const path = (p.panel.params as { path?: string } | undefined)?.path ?? p.panel.id
    return [
      { label: 'Split right', disabled: single, action: () => split(p.api, p.panel, p.group, 'right') },
      { label: 'Split down', disabled: single, action: () => split(p.api, p.panel, p.group, 'below') },
      { label: 'Keep open', action: () => pinPath(path) },
      'separator' as const,
      'close' as const,
      'closeOthers' as const,
      'closeAll' as const,
    ]
  }, [])

  return (
    <div className="wst-dock">
      <DockviewReact
        components={components}
        tabComponents={tabComponents}
        watermarkComponent={Watermark}
        onReady={onReady}
        theme={theme}
        disableFloatingGroups
        getTabContextMenuItems={menu}
      />
    </div>
  )
}

// On a narrow screen one item shows at a time, with a switcher that lays the open
// items out as a grid (docs/frontends/gui.md#workspace).
export function NarrowTabs({ show }: { show: (path: string) => void }) {
  const items = useWorkspace((w) => w.items)
  const focused = useWorkspace((w) => w.focused)
  const [grid, setGrid] = useState(false)
  const front = items.find((i) => i.path === focused) ?? items[0]
  return (
    <div className="wst-narrow">
      <div className="wst-narrow-bar">
        <span className="wst-narrow-title">{front ? <NarrowTitle path={front.path} /> : 'Nothing open'}</span>
        <button className="wst-switcher" onClick={() => setGrid((g) => !g)} aria-label="open items" aria-expanded={grid}>
          {items.length}
        </button>
      </div>
      <div className="wst-item">{front ? <AppRoutes location={front.path} /> : <Watermark />}</div>
      {grid && (
        <div className="wst-grid" role="dialog" aria-label="Open items">
          {items.map((i) => (
            <div key={i.path} className={`wst-card${i.path === front?.path ? ' on' : ''}${i.preview ? ' preview' : ''}`}>
              <button
                className="wst-card-main"
                onClick={() => {
                  setGrid(false)
                  show(i.path)
                }}
              >
                <span className={`wst-icon k-${itemKind(i.path)}`}>{KINDS[itemKind(i.path)].glyph}</span>
                <NarrowTitle path={i.path} />
                <span className="muted wst-card-path">{itemKey(i.path)}</span>
              </button>
              <button className="wst-close" aria-label="close" onClick={() => closePath(i.path)}>
                ×
              </button>
            </div>
          ))}
          {items.length === 0 && <p className="muted">Nothing open.</p>}
        </div>
      )}
    </div>
  )
}

function NarrowTitle({ path }: { path: string }) {
  return <span className="wst-title">{useItemTitle(path)}</span>
}
