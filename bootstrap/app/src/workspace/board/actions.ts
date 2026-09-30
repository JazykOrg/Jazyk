// What a node on the board asks of the board: zoom to itself, open an item beside
// it, close its item.
import { createContext, useContext } from 'react'
import type { Point } from './model'

export interface BoardActions {
  zoomToNode: (id: string) => void
  // Opens `path` as a window: beside the item `from`, or beside `near` (a chip).
  open: (path: string, opts: { from?: string; near?: string }) => void
  close: (path: string) => void
  resize: (path: string, rect: { x: number; y: number; w: number; h: number }) => void
  chipAt: (entity: string) => Point | undefined
}

export const BoardActionsContext = createContext<BoardActions | null>(null)

export function useBoardActions(): BoardActions {
  const a = useContext(BoardActionsContext)
  if (!a) throw new Error('board actions outside the board')
  return a
}
