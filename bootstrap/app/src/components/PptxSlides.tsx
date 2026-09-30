// A `.pptx` rendered in the page as its slides (docs/frontends/gui.md#overview),
// by @aiden0z/pptx-renderer. The overview loads this module only when it shows a
// deck, so the renderer and the chart library it carries stay out of the page's
// first script; the service worker precaches the chunk, so a deck renders offline
// (docs/frontends/wasm.md#offline). No PDF.js: the fallback previews it would draw
// (a SmartArt's embedded PDF) are left out rather than fetched.
import { useEffect, useRef, useState } from 'react'
import { PptxViewer, RECOMMENDED_ZIP_LIMITS } from '@aiden0z/pptx-renderer'

export default function PptxSlides({ bytes }: { bytes: Uint8Array }) {
  const ref = useRef<HTMLDivElement>(null)
  const [state, setState] = useState<{ slides: number; rendered: number } | { error: string } | null>(null)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const abort = new AbortController()
    let viewer: PptxViewer | null = null
    let rendered = 0
    setState(null)
    // The renderer owns its container; a copy of the bytes keeps the caller's intact.
    PptxViewer.open(bytes.slice().buffer, el, {
      zipLimits: RECOMMENDED_ZIP_LIMITS,
      pdfjs: false,
      fitMode: 'contain',
      renderMode: 'list',
      signal: abort.signal,
      onSlideRendered: () => {
        rendered += 1
        if (!abort.signal.aborted) setState((s) => (s && 'slides' in s ? { ...s, rendered } : s))
      },
    }).then(
      (v) => {
        if (abort.signal.aborted) {
          v.destroy()
          return
        }
        viewer = v
        setState({ slides: v.slideCount, rendered })
      },
      (e) => {
        if (!abort.signal.aborted) setState({ error: e instanceof Error ? e.message : String(e) })
      },
    )
    return () => {
      abort.abort()
      viewer?.destroy()
      el.replaceChildren()
    }
  }, [bytes])
  return (
    <div className="product-deck">
      {state && 'error' in state && <p className="v-bad">The deck did not render: {state.error}</p>}
      {state && 'slides' in state && (
        <p className="muted" style={{ margin: '4px 0' }}>
          {state.slides} slide{state.slides === 1 ? '' : 's'}
        </p>
      )}
      {!state && <p className="muted">Rendering the slides…</p>}
      <div ref={ref} className="product-slides" data-rendered={state && 'slides' in state ? state.rendered : 0} />
    </div>
  )
}
