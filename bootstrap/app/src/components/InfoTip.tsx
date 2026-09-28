// An ⓘ that explains in place of a sentence (docs/frontends/gui.md#the-action-button).
// The bubble shows on hover and on keyboard focus at once, never waiting on the
// browser's slow `title` tooltip. Its text lives in an attribute the stylesheet
// draws, so it is not text of the row it sits in: a search for a row's words
// never matches an explanation beside it.
export default function InfoTip({ text, className }: { text: string; className?: string }) {
  return (
    <span className={`info-tip${className ? ` ${className}` : ''}`} aria-label={text} data-tip={text} role="img" tabIndex={0}>
      ⓘ
    </span>
  )
}
