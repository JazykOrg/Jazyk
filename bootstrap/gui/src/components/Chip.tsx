// Severity and verification chips, on the viewer's exact status vocabulary.
export function SevChip({ severity }: { severity: string }) {
  return <span className={`chip sev-${severity}`}>{severity}</span>
}

export function verifyClass(status?: string): string {
  if (status === 'verified') return 'v-ok'
  if (status === 'failing') return 'v-bad'
  if (status?.startsWith('stale')) return 'v-stale'
  return 'v-none'
}

export function VerifyChip({ status }: { status?: string }) {
  if (!status) return null
  return <span className={`chip ${verifyClass(status)}`}>{status}</span>
}
