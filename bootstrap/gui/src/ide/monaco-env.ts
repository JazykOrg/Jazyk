// One place to wire monaco's web workers. Vite emits each `?worker` import as a
// separate same-origin asset and instantiates it with a plain URL: no blob or data
// URL involved, which some Firefox privacy configurations refuse (worker creation
// throws, monaco falls back to a mangled https://[ff00::]/ base, then to the main
// thread, and the resulting freezes stall the whole tab). Language services get
// their label-specific workers; everything else gets the base editor worker.
import type * as monaco from 'monaco-editor'
import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import TsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
import JsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import CssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import HtmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'

;(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
  getWorker: (_id: string, label: string) => {
    switch (label) {
      case 'typescript':
      case 'javascript':
        return new TsWorker()
      case 'json':
        return new JsonWorker()
      case 'css':
      case 'scss':
      case 'less':
        return new CssWorker()
      case 'html':
      case 'handlebars':
      case 'razor':
        return new HtmlWorker()
      default:
        return new EditorWorker()
    }
  },
}
