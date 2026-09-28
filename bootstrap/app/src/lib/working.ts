// The pulsing line at the end of a running transcript: what the agent is doing
// right now, in product words (docs/frontends/gui.md#chat, "Working"). A pending
// tool call names the tool in words; anything else is thinking.

// A tool name as a verb, with what it is called on when the call says
// (`Reading docs/backend.md…`, `Searching Customer…`, `Loading ent:shop…`).
export function toolWords(name: string, detail = ''): string {
  const what = detail.trim()
  const on = what ? ` ${what}` : ''
  switch (name) {
    case 'read_section':
    case 'read_doc':
    case 'read_file':
    case 'read':
      return `Reading${on}…`
    case 'search':
      return `Searching${on}…`
    case 'load':
      return `Loading${on}…`
    case 'expand':
      return `Expanding${on}…`
    case 'unload':
      return 'Unloading…'
    case 'graph_status':
      return 'Checking the loaded set…'
    case 'get_entity':
    case 'get_view':
    case 'diagnostics':
      return `Looking up${on}…`
    case 'upsert_entity':
    case 'update_entity':
      return `Writing the entity${on}…`
    case 'upsert_requirement':
    case 'update_requirement':
    case 'revise_requirement':
      return `Revising the requirement${on}…`
    case 'upsert_view':
    case 'update_view':
      return `Curating the diagram${on}…`
    case 'edit_doc':
    case 'write_doc':
    case 'write':
      return `Editing${on}…`
    case 'report_diagnostic':
      return 'Reporting a problem…'
    case 'mark_goal_done':
      return 'Marking done…'
    case 'mark_goal_failed':
      return 'Marking failed…'
    case 'done':
      return 'Finishing…'
    default:
      return `${name || 'Working'}${on}…`
  }
}

export interface WorkRow {
  kind: string
  text: string
  status?: string
}

// The line for a transcript whose turn runs: the tool the last row is still
// calling, or thinking. A chat tool row's text is the call's title (`search
// Customer`); the first word is the tool. Reasoning streaming in is the work shown
// itself: `Thinking…` is only the placeholder until it arrives, so a transcript
// ending in reasoning has no line (docs/frontends/gui.md#chat).
export function workingLine(rows: WorkRow[]): string | null {
  const last = rows[rows.length - 1]
  if (last && last.kind === 'thought') return null
  if (last && last.kind === 'tool' && last.status === 'pending') {
    const [name, ...rest] = last.text.split(/\s+/)
    return toolWords(name, rest.join(' '))
  }
  return 'Thinking…'
}
