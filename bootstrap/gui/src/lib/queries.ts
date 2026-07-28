// Query hooks: one per endpoint, keyed to match the invalidation map in events.ts.
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import {
  get,
  type DocInfo,
  type DocRecord,
  type Graph,
  type JournalEntry,
  type Project,
  type Status,
  type VerifyRow,
} from './api'

const opts = { placeholderData: keepPreviousData, staleTime: 5_000 }

export const useStatus = () =>
  useQuery({ queryKey: ['status'], queryFn: () => get<Status>('/api/status'), ...opts })

export const useProject = () =>
  useQuery({ queryKey: ['project'], queryFn: () => get<Project>('/api/project'), staleTime: Infinity })

export const useGraph = () =>
  useQuery({ queryKey: ['graph'], queryFn: () => get<Graph>('/api/graph'), ...opts })

export const useDocs = () =>
  useQuery({ queryKey: ['docs'], queryFn: () => get<{ docs: DocInfo[] }>('/api/docs'), ...opts })

export const useCoverage = () =>
  useQuery({
    queryKey: ['coverage'],
    queryFn: () => get<Record<string, DocRecord>>('/api/coverage'),
    ...opts,
  })

export const useJournal = (limit = 50) =>
  useQuery({
    queryKey: ['journal', limit],
    queryFn: () => get<{ entries: JournalEntry[]; generation: number }>(`/api/journal?limit=${limit}`),
    ...opts,
  })

export const useMatrix = () =>
  useQuery({
    queryKey: ['matrix'],
    queryFn: () => get<{ rows: Record<string, VerifyRow>; counts: Record<string, number> }>('/api/verify/matrix'),
    ...opts,
  })

export const useGenPending = () =>
  useQuery({
    queryKey: ['pending', 'gen'],
    queryFn: () => get<{ pending: { entity: string; reason: string; changed: string[] }[] }>('/api/gen/pending'),
    ...opts,
  })

export const useNode = (id: string) =>
  useQuery({
    queryKey: ['node', id],
    queryFn: () => get<Record<string, unknown>>(`/api/entities/${encodeURIComponent(id)}`),
    enabled: id.startsWith('ent:'),
    ...opts,
  })

export const useContextPack = (target: string) =>
  useQuery({
    queryKey: ['node', 'context', target],
    queryFn: () =>
      get<{ pack: string; handles: { handle: string; description: string }[] }>(
        `/api/context?target=${encodeURIComponent(target)}`,
      ),
    ...opts,
  })
