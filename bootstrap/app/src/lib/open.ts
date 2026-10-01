// Whether a project is open (docs/frontends/gui.md#layout), and the one the browser
// remembers (docs/frontends/gui.md#the-open-project). A server always has one; the
// browser backend has none until a demo, a browser project, or a folder is picked,
// and reopens the remembered one on a reload. Undefined while the backend has not
// answered yet or the remembered project is still being reopened.
import { useCapabilities } from './capabilities'
import { useProjects } from './product'
import { useApp } from './store'

const OPEN_KEY = 'jazyk.open'

// The recent entry's id of the project open in this browser, or null.
export function readOpenId(): string | null {
  try {
    return localStorage.getItem(OPEN_KEY)
  } catch {
    return null
  }
}

export function rememberOpen(id: string) {
  try {
    localStorage.setItem(OPEN_KEY, id)
  } catch {
    // not remembered
  }
  useApp.getState().setOpenId(id)
}

export function clearOpen() {
  try {
    localStorage.removeItem(OPEN_KEY)
  } catch {
    // nothing to clear
  }
  useApp.getState().setOpenId(null)
}

export function useProjectOpen(): boolean | undefined {
  const caps = useCapabilities()
  const projects = useProjects()
  const reopening = useApp((a) => a.reopening)
  if (!caps.data) return undefined
  if (caps.data.backend === 'server') return true
  // The remembered project is on its way: the routes wait rather than redirect.
  if (reopening) return undefined
  if (!projects.data) return projects.isError ? false : undefined
  return projects.data.projects.some((p) => p.current)
}
