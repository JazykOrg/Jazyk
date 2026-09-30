// A page rendered as an item (a board window, a tab) navigates through its item,
// not the address bar: every `navigate`, `Link`, and search-param change inside it
// arrives at `onNavigate` with the target path, and so does a click on a plain
// in-app anchor (`#/...`). The item decides: a new page opens beside it, a change of
// its own query stays in it (plans/workspace.md#board). The item's routes match
// from the root, wherever the item is mounted (a board inside a route included).
import { useCallback, useContext, useMemo, type ReactNode } from 'react'
import { createPath, UNSAFE_NavigationContext, UNSAFE_RouteContext, type Navigator, type To } from 'react-router'

export type ItemNavigate = (to: string, opts: { replace: boolean }) => void

function toPath(to: To): string {
  return typeof to === 'string' ? to : createPath(to)
}

const ROOT_ROUTE = { outlet: null, matches: [], isDataRoute: false }

export default function ItemNavigation({ onNavigate, children }: { onNavigate: ItemNavigate; children: ReactNode }) {
  const parent = useContext(UNSAFE_NavigationContext)
  const value = useMemo(() => {
    const up = parent.navigator
    const navigator: Navigator = {
      createHref: (to) => up.createHref(to),
      encodeLocation: up.encodeLocation ? (to) => up.encodeLocation!(to) : undefined,
      // History belongs to the address bar, not to one item.
      go: () => {},
      push: (to) => onNavigate(toPath(to), { replace: false }),
      replace: (to) => onNavigate(toPath(to), { replace: true }),
    }
    return { ...parent, navigator }
  }, [parent, onNavigate])
  const onClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return
      const a = (e.target as Element).closest?.('a')
      const href = a?.getAttribute('href')
      if (!a || !href?.startsWith('#/') || (a.target && a.target !== '_self')) return
      e.preventDefault()
      onNavigate(href.slice(1), { replace: false })
    },
    [onNavigate],
  )
  return (
    <UNSAFE_NavigationContext.Provider value={value}>
      <UNSAFE_RouteContext.Provider value={ROOT_ROUTE}>
        <div className="item-nav" onClick={onClick}>
          {children}
        </div>
      </UNSAFE_RouteContext.Provider>
    </UNSAFE_NavigationContext.Provider>
  )
}
