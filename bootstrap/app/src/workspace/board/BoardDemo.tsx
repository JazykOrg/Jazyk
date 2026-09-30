// A lab page for the Board arrangement (`/lab/board`): the board full screen over
// the app with the project's documents listed to open, so the board is tried and
// tested before the shell's navigator mounts it.
import { useDocs } from '../../lib/queries'
import { useWorkspace } from '../items'
import BoardArrangement from './Board'

export default function BoardDemo() {
  const { data } = useDocs()
  const items = useWorkspace((s) => s.items)
  const openItem = useWorkspace((s) => s.openItem)
  return (
    <div className="board-lab">
      <nav className="board-lab-docs" aria-label="Documents">
        <b>Documents</b>
        {(data?.docs ?? []).map((d) => {
          const path = `/docs/${d.path}`
          const open = items.some((i) => i.path === path)
          return (
            <button key={d.path} className={open ? 'open' : ''} onClick={() => openItem(path)} title={path}>
              {d.path}
            </button>
          )
        })}
        <a href="#/docs">Leave the lab</a>
      </nav>
      <BoardArrangement />
    </div>
  )
}
