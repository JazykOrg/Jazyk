# Project workflows and launch scope

Status: proposed product scope, 2026-10-01. Review this before choosing hosting or
implementing billing. This page takes precedence over the earlier, broader workflow
options in this plan. It does not remove existing features from the application.

## Recommendation

Offer two full project homes: `On this computer` and `In Jazyk Cloud`. Keep a
`Browser playground` for discovery. Present AI as a setting on a project. Present
CLI, the web editor, and IDEs as ways to use that project.

The user chooses a project, not a matrix of backend, storage driver, agent, and
model transport. Each project shows three plain-language facts:

- `Runs on`: this computer, Jazyk Cloud, or this browser.
- `Saved in`: a named folder, Jazyk Cloud, or this browser only.
- `AI`: your provider, Jazyk subscription, your agent, or optional browser AI.

The [clickable prototype](./ux/index.html) illustrates the proposed picker, editor,
subscription switch, IDE path, cloud transfer, and playground promotion. Everything
in it is simulated. It makes no network requests, charges, or project changes.
Open the HTML file directly, or serve its directory over HTTP.

Static previews: [project picker](./ux/projects.png), [subscription switch](./ux/upgrade.png),
[editor context](./ux/workspace.png), and [mobile layout](./ux/mobile.png).

## In, later, and out

`In` means part of the target product, delivered local first and cloud second.
`Later` means deliberately absent from the first paid release or cloud beta.
`Out` means absent from the proposed main workflow. Existing browser users still
need an explicit transition before implementation removes any capability.

| Workflow or option | Decision | Reason and presentation |
|---|---|---|
| Computer project, native Jazyk, local folder | In first | Full compiler. Free without an account when using your own provider or agent. |
| Computer project with Jazyk subscription | In first | AI access changes. Files and compilation stay on the computer. |
| CLI and IDE access through ACP/LSP | In first | Open the same folder and graph through another interface. |
| Cloud project, durable cloud storage, Jazyk subscription | In cloud beta | Full compiler without a local installation. Show compute separately from AI. |
| Browser playground using WASM | In, limited | Explore examples, inspect a sample graph, edit a draft, and export or continue in a full project. |
| WebLLM and Chrome built-in AI | Experimental playground only | Optional reading assistance. Do not offer them as equivalent paid compilation/coding routes. No automatic model download. |
| Browser BYOK and browser subscription | Out of launch workflow | Avoid selling a partial compiler environment and exposing another credential flow. Move to a full project to compile. |
| WASM writing directly into a computer folder | Out of new-project flow | A computer project consistently means native Jazyk. Preserve access/export for existing users during transition. |
| Cloud BYOK, initially one supported provider | Later | Local BYOK remains supported. Defer cloud credentials, brokerage, and another onboarding branch. |
| Mixed model/provider policies per purpose | In, advanced | Show one default AI source. Reveal task overrides only when requested. |
| GitHub import/publish | Later cloud integration | Repository connection belongs under storage details, not among execution locations. Local Git usage works independently. |
| Automatic local/cloud/GitHub synchronization | Out of initial scope | Transfers are explicit. Concurrent copies need conflict and graph-merge semantics. |
| Remote cloud IDE attachment | Later | Keep local ACP/LSP working first. A remote IDE session needs a separate connection and security design. |

The first paid release does not require cloud workspaces. Build the common picker
with computer projects and a playground, then add the cloud home when it works.
Do not show a functional cloud button before the service is available. The prototype
shows the eventual cloud beta so the overall experience can be reviewed now.

## Project picker

Make `Projects` the default home on jazyk.dev. Use one list with columns for name,
run location, saved location, and AI. A local project also shows whether its Jazyk
connection is available. A cloud project shows running or paused. A playground
shows `This browser only` rather than a cloud-save indicator.

Selecting a row opens its details. `Open project` is the primary action. The details
offer `Change AI`, `Use in an IDE`, and an explicit transfer where supported.
Search and a location filter help a long list. Do not require separate apps, URLs,
or accounts for each location.

List cloud projects belonging to the signed-in account and computer projects known
to the connected native Jazyk or this browser. Login does not scan a computer or
upload its local project list. A recent local entry on another machine cannot be
opened until that machine is explicitly connected. First release: current computer
only, no remote access to an arbitrary personal laptop.

`New project` offers:

- `On this computer`: connect or install Jazyk, choose a folder, open it. Use the
  existing provider by default. No signup requirement.
- `In Jazyk Cloud`: sign in, review cloud entitlement and spending limit, create.
  `Jazyk subscription` is the default AI source.
- A secondary `Try a browser playground` action. No account or installation.

An existing-folder action must also go through native Jazyk. If disconnected, show
`Run jazyk gui` and connection status. Preserve the folder and pending action while
the user connects. Do not silently fall back to WASM with different capabilities.

## Inside a project

Keep one compact, clickable context strip in the editor:

`Runs on this computer · Saved in ~/Projects/erp · AI: Your provider`

`Projects` opens the same picker. Opening AI settings never changes the backend.
Opening storage details shows the actual folder/cloud checkpoint, optional repository
connection, export, and last durable save. Use technical names such as WASM and ACP
inside details where they help, not in the main project choice.

Display saving status independently of model or connection status:

- Computer: `Saved to folder`. This is not a backup claim. A Git remote can be shown
  separately with its last published revision.
- Cloud: `Saved to workspace`, then `Backed up at <time>` after a durable checkpoint.
  `Unpublished changes` can coexist with `Backed up` for a repository connection.
- Browser: `Saved in this browser only`. The current implementation uses IndexedDB
  for projects. LocalStorage holds settings/recent metadata. Neither is cloud storage.
  If persistent browser storage fails, show `Temporary, lost when this tab closes`.

Use capability-based actions. A playground offers edit, inspect, download, and
`Continue as a full project`. It explains that full compilation, code execution,
and IDE access need the computer or cloud home. After manual edits, label its sample
graph as not updated. Do not present a stale graph as the result of those edits.

## Subscription journey

### Computer or web editor

1. Open a folder with native Jazyk. Keep the existing provider and graph.
2. Choose `AI` then `Use Jazyk subscription`.
3. Sign in and choose a subscription only if needed. Review the model allowance,
   selected policy, per-run limit, and data sent for inference.
4. Confirm the AI change. Show that execution and storage still use this computer.
5. The next eligible model session uses Jazyk. An active session finishes with its
   original route. Explicit environment/CLI overrides are surfaced before a run.

Login alone does not opt a project into paid inference. Never replace LocalRouter
or send account tokens to an arbitrary configured endpoint. A subscription sends
selected model context to the managed service; it does not upload the whole project
as a cloud workspace. Show both facts before activation.

Cancellation or an exhausted allowance keeps the folder and graph accessible.
Offer an explicit switch to the user's provider. Do not switch bills automatically,
silently charge overages, or silently start cloud compute.

### CLI or IDE

The proposed `jazyk login` command opens account authorization and returns to the
terminal. This command is planned, not an existing implemented feature. Selecting a
managed profile is a separate explicit step, available in CLI and project settings.
The user does not need to edit documents on jazyk.dev to subscribe.

- `jazyk acp` exposes Jazyk as an agent to a compatible IDE. The native compiler
  reads the same folder and graph. Its embedded worker can use the selected Jazyk
  subscription profile.
- `jazyk lsp` provides diagnostics, navigation, and graph reading. Passive language
  features do not need a subscription. An explicit action that starts model work
  follows the selected route and its spending controls.
- An external agent configured under `[executors]` owns its model configuration and
  bill unless explicitly integrated with the managed gateway. Show `Your agent`.
  An IDE's unrelated assistant is not covered by buying Jazyk.

Do not change an external executor when the user logs in. Offer switching supported
tasks to the embedded worker as a separate choice. Per-purpose mixed policies show
`Mixed` with a breakdown, rather than claiming that every task uses the subscription.

### Browser playground

Let the visitor explore before offering a subscription. The next action is
`Continue as a full project`, with computer and cloud destinations. Export/import
the exact source and any compatible persistent graph state. For a fresh draft with
no graph, create the graph on its first real compilation.

On the computer, own-provider use remains free and a subscription is optional.
In the cloud, review both AI and compute before creating a paid workspace. Avoid a
subscription checkout that leaves the user in a browser environment unable to run
the requested task.

## Moving execution and storage

`Use Jazyk subscription` changes only AI. `Move to cloud` changes the project home.
Keep these actions separate in wording and placement.

For the first cloud beta, use a guided export/import flow, not live synchronization:

1. Finish or cancel active work and export a consistent checkpoint with docs, graph,
   decisions, and ledgers. Exclude credentials and transient process state.
2. Review what will be uploaded, paid cloud usage, and unsupported local dependencies.
   Local endpoints and local executables need replacement or explicit setup.
3. Import and validate the checkpoint. Preserve graph ids on a compatible version.
   If the compiler cannot restore that version, stop and explain the consequence;
   do not silently rebuild it. The current compiler archives mismatched graph formats.
4. Open the destination only after a successful restore. Label the original as a
   local copy with no sync. Do not delete it. The picker points to the new working
   project; the user can still explicitly reopen the original as an independent copy.

The reverse path is download and open on the computer. A cloud subscription lapse
must still allow export within the disclosed retention period. Account/project ids
may change on a copy; graph node ids do not. Do not promise a distributed write lease
that prevents someone independently running the original folder.

## GitHub and graph identity

GitHub can preserve identities and decisions. The prior wording was too absolute.
The issue is which files are committed. A source-only repository cannot recover
state it never stored. Jazyk currently ignores `.jazyk/` by default.

Two valid repository policies are:

- Source in Git, graph in Jazyk checkpoints. This is the proposed cloud default.
- Source plus an explicitly included, consistent durable graph checkpoint in Git.
  That can preserve identities, decrees, diagnostics, and history. Exclude secrets,
  locks, credentials, caches, and other transient state.

A raw merge of two graph histories is not automatically semantically valid. Git can
store snapshots; the compiler still needs a policy for divergent graph edits and
version compatibility. This is an integration problem, not a GitHub storage limit.
See [storage alternatives](./storage.md#github-can-store-the-graph).

## Review and implementation order

1. Review the picker and upgrade prototype. Decide the browser scope and deferred
   cloud BYOK before building more provider or storage choices.
2. Specify shared project identity, capabilities, location labels, and AI settings in
   `docs/web/`. Update the GUI and WASM specs to describe the accepted transition.
   Preserve existing browser projects and exports until migration is available.
3. Unify computer project entry and IDE/CLI AI settings. Add managed inference.
4. Add the cloud home and durable export/import. Add GitHub publishing afterward.

Estimated incremental product work: 3–5 engineering days for the picker/context and
native connection flow, 2–4 for shared AI settings and upgrade/IDE states, and 2–4
for playground promotion and legacy-entry handling. These 7–13 days overlap the
earlier web/account/storage estimates. They are not an additional hosted platform
estimate. A short design review and prototype precede those implementation days.

Acceptance examples:

- Local BYOK users open and work without login. Subscription activation leaves the
  folder path, backend, graph ids, and external executor choices intact.
- A disconnected local project asks for its connection rather than becoming a
  browser or cloud project. Opening a paused cloud project shows compute startup.
- IDE-only users can authorize and select managed AI from the terminal.
- The playground always says where it saves and offers a portable exit.
- A GitHub badge never implies the graph is in Git unless it actually is.

The remaining product decisions are narrow: accept these two full homes, accept the
limited playground and removal of new WASM-folder onboarding, and defer cloud BYOK
and GitHub integration. Model names, sandbox vendors, and retention prices can be
resolved after those workflow choices.
