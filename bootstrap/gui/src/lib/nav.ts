// Inspector selection and cross-pane navigation. The inspector is the ?node=
// search param, preserved on the current center; opening a node never navigates
// away (docs/frontends/gui.md#inspector).
import { useCallback } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router'

export function useInspector(): {
  node: string | null
  openNode: (id: string) => void
  closeNode: () => void
} {
  const [sp, setSp] = useSearchParams()
  const node = sp.get('node')
  const openNode = useCallback(
    (id: string) => {
      setSp(
        (p) => {
          const next = new URLSearchParams(p)
          next.set('node', id)
          return next
        },
        { replace: false },
      )
    },
    [setSp],
  )
  const closeNode = useCallback(() => {
    setSp(
      (p) => {
        const next = new URLSearchParams(p)
        next.delete('node')
        return next
      },
      { replace: true },
    )
  }, [setSp])
  return { node, openNode, closeNode }
}

// A same-page href that adds ?node= to the current location (for Link targets).
export function useNodeHref(): (id: string) => string {
  const loc = useLocation()
  return useCallback(
    (id: string) => {
      const next = new URLSearchParams(loc.search)
      next.set('node', id)
      return `${loc.pathname}?${next.toString()}`
    },
    [loc.pathname, loc.search],
  )
}

export function docHref(doc: string, section?: string, quote?: string): string {
  const params = new URLSearchParams()
  if (section) params.set('section', section)
  if (quote) params.set('quote', quote)
  const qs = params.toString()
  return `/files/docs/${doc}${qs ? `?${qs}` : ''}`
}

export function delivHref(path: string, site?: string): string {
  const qs = site ? `?site=${encodeURIComponent(site)}` : ''
  return `/files/deliverable/${path}${qs}`
}

export function useOpenActivity(): (run?: string) => void {
  const navigate = useNavigate()
  const loc = useLocation()
  return useCallback(
    (run?: string) => {
      const next = new URLSearchParams(loc.search)
      if (run) next.set('run', run)
      navigate(`${loc.pathname}?${next.toString()}`)
    },
    [navigate, loc.pathname, loc.search],
  )
}
