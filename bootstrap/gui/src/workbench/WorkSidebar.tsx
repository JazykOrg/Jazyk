// The work sidebar: the generation worklist and the verification rollup
// (docs/frontends/gui.md#work). Rows open the center views.
import { NavLink } from 'react-router'
import { useGenPending, useMatrix } from '../lib/queries'
import { verifyClass } from '../components/Chip'

export default function WorkSidebar() {
  const pending = useGenPending()
  const matrix = useMatrix()
  const counts = matrix.data?.counts ?? {}

  return (
    <>
      <div className="wb-explorer-label" style={{ paddingTop: 10 }}>
        generation
      </div>
      <NavLink to="/work" end className={({ isActive }) => `wb-list-row${isActive ? ' active' : ''}`}>
        worklist{' '}
        <span className="sub">{pending.data ? `${pending.data.pending.length} pending` : '…'}</span>
      </NavLink>
      {(pending.data?.pending ?? []).map((p) => (
        <NavLink
          key={p.entity}
          to={`/work/gen/${encodeURIComponent(p.entity)}`}
          className={({ isActive }) => `wb-list-row mono${isActive ? ' active' : ''}`}
          style={{ paddingLeft: 24 }}
        >
          {p.entity} <span className="sub">{p.reason}</span>
        </NavLink>
      ))}
      <div className="wb-explorer-label" style={{ paddingTop: 12 }}>
        verification
      </div>
      <NavLink to="/work/verify" className={({ isActive }) => `wb-list-row${isActive ? ' active' : ''}`}>
        matrix{' '}
        <span className="sub">
          {Object.entries(counts).map(([st, n]) => (
            <span key={st} className={verifyClass(st)} style={{ marginRight: 6 }}>
              {n} {st}
            </span>
          ))}
        </span>
      </NavLink>
    </>
  )
}
