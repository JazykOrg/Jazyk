# Editor protocol

The local HTTP API and the WebAssembly API share an editor protocol. The editor
and engine may release independently. The protocol version is independent of the
engine package version and the graph storage version.

## Discovery

`GET /api/ping` includes an integer `protocol` field. The current version is `1`.
The response retains `app`, `version`, and `root`. `app` is `jazyk-gui` for the
local HTTP API and `jazyk-wasm` for WebAssembly. `version` is the engine package
version. It does not decide editor compatibility.

E.g.:

```json
{
  "app": "jazyk-gui",
  "version": "0.2.0",
  "protocol": 1,
  "root": "/home/user/project"
}
```

The ping reads metadata only. It does not load or migrate the graph. It remains
available without the local session token. Other API routes retain their token
checks. The local listener remains bound to loopback. The admitted editor origin
and the CORS rules do not change when the protocol version is added.

## Capabilities

`GET /api/capabilities` includes the same `protocol` field. The existing capability
descriptor remains authoritative for supported operations. Its `jobs`, `docs`,
`events`, `lsp`, `agent`, `model`, `rendering`, and `persistence` fields describe the
active backend. A matching protocol does not imply every backend has every
capability. Clients ignore additional descriptor fields they do not understand.

The browser adapter adds the protocol to its capability descriptor. It obtains
the engine protocol from the WebAssembly ping before activating the backend.

## Compatibility

The editor supports protocol `1`. An absent `protocol` field means the legacy
version `1`, so existing local engines remain usable. An explicit unsupported
integer or a malformed field is incompatible. It never falls back to the legacy
version. A legacy native API that has no capability route keeps the existing
capability fallback on HTTP `404`.

The editor checks compatibility before changing the active backend, its session
token, project caches, or unsaved document state. A refusal names the incompatible
engine and retains the current project. It does not reset, migrate, regenerate,
or delete graph data. Reconnecting to a compatible engine resumes the existing
workflow. A new editor installation does not require an engine upgrade.

Additive optional fields preserve protocol `1`. Removing an endpoint, changing a
required field's type, or changing an existing operation incompatibly requires a
new protocol version. Release checks exercise the current descriptor, the legacy
response without a version, and an explicitly incompatible response.

The [local API](gui.md) and the [WebAssembly engine](wasm.md) define the transports.
The protocol does not replace their authorization, persistence, or capability
contracts.
