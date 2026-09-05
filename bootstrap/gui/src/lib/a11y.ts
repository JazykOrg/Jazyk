// Keyboard reach for the app's click targets that are not links or buttons: a
// tree row, a breadcrumb, a chip, a card. Spread the result onto the element so
// it takes focus in the tab order and activates on Enter or Space like a button.
import type { KeyboardEvent, MouseEvent } from 'react'

export function pressable<T extends Element>(onClick: (e: MouseEvent<T> | KeyboardEvent<T>) => void) {
  return {
    role: 'button' as const,
    tabIndex: 0,
    onClick,
    onKeyDown: (e: KeyboardEvent<T>) => {
      // A key on a button or link nested inside (a card's actions) is theirs.
      if (e.target !== e.currentTarget) return
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault()
        onClick(e)
      }
    },
  }
}
