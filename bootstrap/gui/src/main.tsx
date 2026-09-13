import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { BrowserRouter } from 'react-router'
import App from './App'
import { initToken, tokenParam } from './lib/api'
import { installServerBackend } from './lib/backend'
import { startEventStream } from './lib/events'
import { applyTheme } from './lib/store'
import './tokens.css'
import './app.css'

initToken()
// The app talks to the native server unless a demo entry installed the wasm
// backend first (docs/frontends/wasm.md#one-api-two-transports).
installServerBackend(() => {
  const p = tokenParam()
  return p ? p.slice('token='.length) : null
})
applyTheme()

const qc = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
})

startEventStream(qc)

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={qc}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </React.StrictMode>,
)
