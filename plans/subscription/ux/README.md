# Workflow prototype

Open [index.html](./index.html) in a browser. The HTML, CSS, and JavaScript are
self-contained local assets with no package install or build step. All data is
simulated in memory. Reload or `Reset demo` restores the examples.

To serve it locally from the repository root:

```sh
python3 -m http.server 8765 --bind 127.0.0.1 --directory plans/subscription
```

Then open `http://127.0.0.1:8765/ux/`.

## Review paths

- Select `Acme ERP`, change AI, and simulate subscription activation. The computer
  and folder stay the same. Open the editor to see its context strip.
- Select `Team handbook` to see external-agent billing. Account activation preserves
  that agent. AI settings offer a separate change to the embedded worker.
- Open `Use in an IDE` for the ACP/LSP and proposed CLI login journey.
- Resume `Field notes` to see the separate compute review.
- Open the playground and continue on the computer or in the cloud. The original
  draft remains available with no synchronization.
- Open connection details and simulate a disconnect. Opening a local project asks
  for native Jazyk and preserves the selected folder.
- Open `New project` for the two full project homes and secondary playground path.
- Open `In / out` for the proposed release boundary.

The document editor, model calls, filesystem operations, exports, account signup,
payments, and cloud provisioning are illustrative. No production app files are
changed by this prototype. Pricing is illustrative and cloud prices are unresolved.

## Previews

- [Project picker](./projects.png).
- [Subscription switch](./upgrade.png).
- [Editor context](./workspace.png).
- [Mobile layout](./mobile.png).

The [workflow proposal](../workflows.md) defines the scope and transition rules.
