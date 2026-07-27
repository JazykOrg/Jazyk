// Cmd/Ctrl-K search over the graph: type, pick a hit, land on /n/:id.
import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { get } from '../lib/api'
import '../routes/routes.css'

interface Hit {
  id: string
  name: string
  definition: string
}

export default function CommandPalette() {
  const [open, setOpen] = useState(false)
  const [q, setQ] = useState('')
  const [hits, setHits] = useState<Hit[]>([])
  const [sel, setSel] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const timer = useRef<number | null>(null)
  const navigate = useNavigate()

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setOpen((v) => !v)
      }
      if (e.key === 'Escape') setOpen(false)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    if (open) {
      setQ('')
      setHits([])
      setSel(0)
      // Focus after the overlay mounts.
      window.setTimeout(() => inputRef.current?.focus(), 0)
    }
  }, [open])

  // Debounced search, 150ms.
  useEffect(() => {
    if (!open) return
    if (timer.current !== null) window.clearTimeout(timer.current)
    if (!q.trim()) {
      setHits([])
      return
    }
    timer.current = window.setTimeout(() => {
      get<{ hits: Hit[] }>(`/api/search?q=${encodeURIComponent(q)}`)
        .then((r) => {
          setHits(r.hits)
          setSel(0)
        })
        .catch(() => setHits([]))
    }, 150)
    return () => {
      if (timer.current !== null) window.clearTimeout(timer.current)
    }
  }, [q, open])

  if (!open) return null

  const go = (id: string) => {
    setOpen(false)
    navigate(`/n/${encodeURIComponent(id)}`)
  }

  return (
    <>
      <div className="palette-backdrop" onClick={() => setOpen(false)} />
      <div className="palette">
        <input
          ref={inputRef}
          type="search"
          placeholder="search the graph"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              setSel((s) => Math.min(s + 1, hits.length - 1))
            } else if (e.key === 'ArrowUp') {
              e.preventDefault()
              setSel((s) => Math.max(s - 1, 0))
            } else if (e.key === 'Enter' && hits[sel]) {
              go(hits[sel].id)
            }
          }}
        />
        <div className="palette-hits">
          {q.trim() && hits.length === 0 && <p className="muted">no hits</p>}
          {hits.map((h, i) => (
            <div
              key={h.id}
              className={`palette-hit ${i === sel ? 'sel' : ''}`}
              onMouseEnter={() => setSel(i)}
              onClick={() => go(h.id)}
            >
              <span className="mono">{h.id}</span> <b>{h.name}</b>{' '}
              <span className="muted oneline" style={{ display: 'inline-block', maxWidth: '55%', verticalAlign: 'bottom' }}>
                {h.definition}
              </span>
            </div>
          ))}
        </div>
      </div>
    </>
  )
}
