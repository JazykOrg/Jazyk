// Query hooks: one per endpoint, keyed to match the invalidation map in events.ts.
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import {
  get,
  getOr404,
  type BenchmarkModels,
  type BenchmarkTable,
  type Deliverable,
  type DocInfo,
  type DocRecord,
  type FeedbackEntry,
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

export const useFeedback = (limit = 200) =>
  useQuery({
    queryKey: ['feedback', limit],
    queryFn: () => get<{ entries: FeedbackEntry[] }>(`/api/feedback?limit=${limit}`),
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

export const useBenchmarks = () =>
  useQuery({ queryKey: ['benchmarks'], queryFn: () => get<BenchmarkTable>('/api/benchmarks'), ...opts })

// The endpoint's own model listing; an empty baseUrl asks about the resolved default.
export const useBenchmarkModels = (baseUrl: string) =>
  useQuery({
    queryKey: ['benchmarks', 'models', baseUrl],
    queryFn: () =>
      get<BenchmarkModels>(`/api/benchmarks/models${baseUrl ? `?baseUrl=${encodeURIComponent(baseUrl)}` : ''}`),
    ...opts,
  })

export const useNode = (id: string) =>
  useQuery({
    queryKey: ['node', id],
    queryFn: () => get<Record<string, unknown>>(`/api/entities/${encodeURIComponent(id)}`),
    enabled: id.startsWith('ent:'),
    ...opts,
  })

export const useDeliverable = () =>
  useQuery({ queryKey: ['deliverable'], queryFn: () => get<Deliverable>('/api/deliverable'), ...opts })

// The last reconciled text; null when the document never reconciled.
export const useDocBaseline = (path: string) =>
  useQuery({
    queryKey: ['docs', 'baseline', path],
    queryFn: () => getOr404<{ path: string; text: string; hash: string }>(`/api/docs/baseline?path=${encodeURIComponent(path)}`),
    enabled: path !== '',
    ...opts,
  })

// The file before the last generation rewrote it; null when generation never did.
export const useDelivBaseline = (path: string) =>
  useQuery({
    queryKey: ['deliverable', 'baseline', path],
    queryFn: () =>
      getOr404<{ path: string; text?: string; binary?: boolean }>(`/api/deliverable/baseline?path=${encodeURIComponent(path)}`),
    enabled: path !== '',
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
