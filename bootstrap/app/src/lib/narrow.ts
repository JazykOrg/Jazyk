// Whether the screen is phone-narrow (docs/frontends/gui.md#layout). The same
// breakpoint as the stylesheet's narrow rules.
import { useSyncExternalStore } from 'react'

const QUERY = '(max-width: 760px)'

export function useNarrow(): boolean {
  return useSyncExternalStore(
    (notify) => {
      const m = window.matchMedia(QUERY)
      m.addEventListener('change', notify)
      return () => m.removeEventListener('change', notify)
    },
    () => window.matchMedia(QUERY).matches,
  )
}
