// The workspace switch (docs/frontends/gui.md#workspace-experimental): Lenses, Tabs, or
// Board, remembered per browser. A segmented control in the top bar while the
// workspace is on, a choice in Settings always.
import { useWorkspace, type Arrangement } from './items'

const CHOICES: { value: Arrangement; label: string; title: string }[] = [
  { value: 'lenses', label: 'Lenses', title: 'Docs, Model, and Product as separate places' },
  { value: 'tabs', label: 'Tabs', title: 'open items as tabs that split, with Connections under them' },
  { value: 'board', label: 'Board', title: 'open items as windows on a table top, with the graph between them' },
]

export default function ArrangementSwitch() {
  const arrangement = useWorkspace((w) => w.arrangement)
  const set = useWorkspace((w) => w.setArrangement)
  return (
    <div className="ws-arrange" role="radiogroup" aria-label="Workspace">
      {CHOICES.map((c) => (
        <button
          key={c.value}
          role="radio"
          aria-checked={arrangement === c.value}
          className={arrangement === c.value ? 'on' : ''}
          title={c.title}
          onClick={() => set(c.value)}
        >
          {c.label}
        </button>
      ))}
    </div>
  )
}

export function ArrangementSetting() {
  const arrangement = useWorkspace((w) => w.arrangement)
  const set = useWorkspace((w) => w.setArrangement)
  return (
    <section className="settings-workspace">
      <h1>Workspace</h1>
      <div className="settings-grid">
        <label title="Lenses is the shipped layout. Tabs and Board are experiments: one set of open items, Connections under the center, no inspector.">
          Workspace
          <select aria-label="Workspace" value={arrangement} onChange={(e) => set(e.target.value as Arrangement)}>
            {CHOICES.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="muted">Remembered in this browser. Tabs and Board are experiments; Lenses stays the default until one is picked.</p>
    </section>
  )
}
