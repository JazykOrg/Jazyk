// Whether a project is open (docs/frontends/gui.md#layout). A server always has
// one; the browser backend has none until a demo or a folder is picked, and none
// again after a reload. Undefined while the backend has not answered yet.
import { useCapabilities } from './capabilities'
import { useProjects } from './product'

export function useProjectOpen(): boolean | undefined {
  const caps = useCapabilities()
  const projects = useProjects()
  if (!caps.data) return undefined
  if (caps.data.backend === 'server') return true
  if (!projects.data) return projects.isError ? false : undefined
  return projects.data.projects.some((p) => p.current)
}
