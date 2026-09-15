// Query hooks for the product surface's endpoints: the Inbox rollup and the
// projects list (docs/frontends/gui.md#inbox, docs/frontends/gui.md#home).
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { get, type Verdict } from './api'

export interface InboxRollup {
  generation: number
  questions: number
  proposals: number
  problems: Record<string, number>
  pending: { docs: string[]; entities: number }
  drafts: number
  modes: { compile: string; generate: string }
  total: number
}

export const useInbox = () =>
  useQuery({
    queryKey: ['inbox'],
    queryFn: () => get<InboxRollup>('/api/inbox'),
    placeholderData: keepPreviousData,
    staleTime: 5_000,
    retry: 0,
  })

export interface ProjectSummary {
  root: string
  name?: string
  shape?: string
  generation?: number
  verdict?: Verdict
  diagnostics?: Record<string, number>
  openedAt?: number
  current?: boolean
  missing?: boolean
}

export const useProjects = () =>
  useQuery({
    queryKey: ['projects'],
    queryFn: () => get<{ projects: ProjectSummary[] }>('/api/projects'),
    staleTime: 30_000,
    retry: 0,
  })
