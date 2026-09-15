// A file under the out directory in the center pane, read-only: a generated page
// on its markdown preview with a source toggle, a .png as the image, a .svg inline
// with its anchors live, anything else as text
// (docs/frontends/gui.md#markdown-preview).
import { useEffect, useRef, useState } from 'react'
import { useLocation, useParams } from 'react-router'
import { EditorState } from '@codemirror/state'
import { EditorView, lineNumbers } from '@codemirror/view'
import { useOutText, useProject } from '../lib/queries'
import { outFileUrl } from '../lib/mdlinks'
import MarkdownView from '../components/MarkdownView'
import OutSvg from '../components/OutSvg'
import '../ide/ide.css'
import '../components/markdown.css'

function isImage(path: string): boolean {
  return /\.(svg|png)$/i.test(path)
}

function isSvg(path: string): boolean {
  return /\.svg$/i.test(path)
}

function isMarkdown(path: string): boolean {
  return /\.md$/i.test(path)
}

// Plain read-only text in the editor chrome, for a .puml or a shard.
function ReadOnlyText({ text }: { text: string }) {
  const divRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const view = new EditorView({
      parent: divRef.current!,
      state: EditorState.create({
        doc: text,
        extensions: [
          lineNumbers(),
          EditorView.editable.of(false),
          EditorState.readOnly.of(true),
          EditorView.lineWrapping,
        ],
      }),
    })
    return () => view.destroy()
  }, [text])
  return (
    <div className="ide-editor-host">
      <div ref={divRef} className="ide-editor" />
    </div>
  )
}

export default function OutFile() {
  const params = useParams()
  const loc = useLocation()
  const path = params['*'] ?? ''
  const projectQ = useProject()
  const image = isImage(path)
  const markdown = isMarkdown(path)
  const textQ = useOutText(image ? '' : path)
  const [source, setSource] = useState(false)

  // The toggle is per file: a new page opens rendered.
  useEffect(() => setSource(false), [path])

  if (!path) return <p className="empty ide-pad">select a generated page</p>
  const out = projectQ.data?.out
  if (!out) return <p className="empty ide-pad">loading…</p>

  return (
    <>
      <div className="ide-topbar">
        <span className="mono">{path}</span>
        <span className="muted">read-only</span>
        {markdown && (
          <div className="ide-topbar-right row">
            <button
              className={source ? 'btn-on' : ''}
              title={source ? 'back to the rendered page' : 'the markdown as written'}
              onClick={() => setSource((v) => !v)}
            >
              source
            </button>
          </div>
        )}
      </div>
      {image ? (
        <div className="out-image">{isSvg(path) ? <OutSvg rel={path} alt={path} /> : <img src={outFileUrl(path)} alt={path} />}</div>
      ) : textQ.isError ? (
        <p className="error-inline ide-pad">{textQ.error.message}</p>
      ) : textQ.data === undefined ? (
        <p className="empty ide-pad">loading…</p>
      ) : markdown && !source ? (
        <MarkdownView text={textQ.data} baseAbs={`${out}/${path}`} fragment={loc.hash.replace(/^#/, '')} />
      ) : (
        <ReadOnlyText text={textQ.data} />
      )}
    </>
  )
}
