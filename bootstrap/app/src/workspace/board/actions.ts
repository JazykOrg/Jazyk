// What a node on the board asks of the board: zoom to itself, open an item beside
// it, focus or close its item, resize its window.
import { createContext, useContext } from 'react'

export interface BoardActions {
  zoomToNode: (id: string) => void
  // A portal's click: focus the node at the far end of a link.
  go: (id: string) => void
  // Opens `path` as a window: beside the item `from`, or beside `near` (a chip).
  open: (path: string, opts: { from?: string; near?: string }) => void
  close: (path: string) => void
  resize: (path: string, rect: { x: number; y: number; w: number; h: number }) => void
  focus: (path: string) => void
}

export const BoardActionsContext = createContext<BoardActions | null>(null)

export function useBoardActions(): BoardActions {
  const a = useContext(BoardActionsContext)
  if (!a) throw new Error('board actions outside the board')
  return a
}
