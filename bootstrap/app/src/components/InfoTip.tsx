// An explanation behind an info icon, never a sentence in the way
// (docs/frontends/gui.md#the-action-button): the icon sits beside what it explains
// and the text is its tooltip.
export default function InfoTip({ text, className }: { text: string; className?: string }) {
  return (
    <span className={`info-tip${className ? ` ${className}` : ''}`} title={text} aria-label={text} role="img" tabIndex={0}>
      ⓘ
    </span>
  )
}
