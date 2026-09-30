// The Board arrangement (docs/frontends/gui.md#workspace-experimental): a placeholder
// until the table top lands.
export default function BoardArrangement() {
  return (
    <div className="ws-scroll">
      <div className="empty-state" style={{ marginTop: 60 }}>
        <b>Board is coming</b>
        Switch to Tabs to work with the open items meanwhile.
      </div>
    </div>
  )
}
