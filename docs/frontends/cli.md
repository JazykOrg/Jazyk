# CLI

A project opens all [document sets](../compiler/document-sets.md). Graph operations address one set
explicitly. Each set owns `.jazyk/<set-id>/`; project file operations share the
physical files and report every membership. The document-set contract defines
configuration, aggregate reads, management commands, and grouped compilation.

The CLI is the command line frontend over the [compiler](../compiler/compiler.md). It runs the
build, inspects the [graph store](../compiler/graph.md) and the
[goal board](../compiler/reconciler.md#goal-derivation), and hosts the other frontends.

Install through npm or a native archive as described in [distribution](distribution.md).

## Commands

### jazyk

Bare `jazyk`, no command, is the launcher, not the help. It lists every way to work
on the project in three groups and starts the one picked:

- Web: the jazyk.dev page over the local API.
- IDEs: the editor with the language server ([LSP](./lsp.md)) where the editor has
  the Jazyk extension, and the Jazyk agent ([ACP](./acp.md)) where the editor hosts
  ACP agents.
- Coding agents: the agent with jazyk's tools over [MCP](./mcp.md).

Inside a project it prints the [status](#jazyk-status) overview first: one line per
document set. The list follows: a heading per group, then one row per
choice with its short name and full name. Only what is installed is listed: an IDE
or a coding agent whose program jazyk cannot find has no row (its short name still
works as a command, below). The native terminal menu supports Windows console events.
The up and down arrows (or `j` and `k`) move, a
letter jumps to the next row whose name starts with it, enter starts the choice, and
`q`, escape, or control-C quit with exit `0`. The cursor starts on the first installed
IDE. A list taller than the terminal scrolls with the cursor.

Every short name is also a command: `jazyk claude` starts Claude Code without the
menu. Picking a row and typing its name do the same thing: set the choice up, then
start it. The setup is idempotent. An entry that is already current is left alone;
a stale one (another command, other arguments) is corrected in place; every file is
merged, keeping its comments, formatting, and other entries. Each written or corrected
file is named on its own line. The `jazyk` command in every entry is the name the user
runs when it resolves to this binary on `PATH`, the absolute path otherwise (the
[registration rule](./acp.md#registration)). Every MCP entry serves
`jazyk mcp all`, every lifecycle toolset in one serving
([toolsets](./mcp.md#toolsets)).

| choice | name | sets up | then runs |
|---|---|---|---|
| `gui` | jazyk.dev web UI | nothing | [`jazyk gui`](#jazyk-gui) |
| `vscode` | Visual Studio Code | the Jazyk extension (LSP), then the [agents step](#ide-agents) (the Jazyk agent through the ACP Client extension `formulahendry.acp-client` and [`jazyk acp install vscode`](#jazyk-acp)) | `code <root>` |
| `cursor` | Cursor | the Jazyk extension (LSP), then the agents step (the Jazyk agent through the ACP Client extension from Open VSX and `jazyk acp install cursor`) | `cursor <root>` |
| `zed` | Zed | the Zed extension (LSP), then the agents step (the Jazyk agent through `jazyk acp install zed`) | `zed <root>` |
| `idea` | IntelliJ IDEA | the shared JetBrains plugin with LSP4IJ (LSP), then the agents step (the Jazyk agent through `jazyk acp install jetbrains`) | the IDE on `<root>` |
| `phpstorm` | PhpStorm | the same | the same |
| `webstorm` | WebStorm | the same | the same |
| `pycharm` | PyCharm | the same | the same |
| `goland` | GoLand | the same | the same |
| `rustrover` | RustRover | the same | the same |
| `clion` | CLion | the same | the same |
| `rider` | Rider | the same | the same |
| `rubymine` | RubyMine | the same | the same |
| `claude` | Claude Code | `.mcp.json`, and `jazyk` in `enabledMcpjsonServers` of `.claude/settings.local.json` | `claude` |
| `codex` | Codex CLI | `.codex/config.toml` (`[mcp_servers.jazyk]`) | `codex` |
| `gemini` | Gemini CLI | `.gemini/settings.json` | `gemini` |
| `opencode` | OpenCode | `opencode.json` (`opencode.jsonc` when that is the file the project has) | `opencode` |
| `pi` | Pi | `.pi/mcp.json` | `pi` |
| `droid` | Factory Droid | `.factory/mcp.json` | `droid` |

What each choice needs to know about its program:

- Zed and the JetBrains IDEs use the [editor adapters](./lsp.md#editor-setup).
  The launcher preserves existing project settings, installs a built adapter when
  available, and names any remaining installation or restart step. Missing local
  artifacts do not prevent the editor or ACP agent from opening.

- The Jazyk extension is installed from the newest `jazyk-<version>.vsix` in the
  checkout the binary was built in (`bootstrap/editors/vscode/`), when the editor
  lists no `jazyk.jazyk` or an older version of it. A binary with no checkout beside
  it says how to build the extension and goes on: the editor still starts, without
  the language server.
- A JetBrains IDE starts through its Toolbox script (`idea`, `phpstorm`, ...) on
  `PATH` or in the Toolbox scripts folder, else, on macOS, through its application
  bundle in `/Applications` or `~/Applications`. All JetBrains IDEs share one ACP
  registration (`~/.jetbrains/acp.json`).
- Claude Code, Codex, and Pi read project configuration only in a folder the person
  trusts, and each asks for that trust on its first start in the folder. Claude Code
  honors the approval in `.claude/settings.local.json` after it, so it asks nothing
  more. Every entry is project-scoped: no choice writes a coding agent's global MCP
  configuration.

Outside a project (no `jazyk.toml` above the working directory) the list is the same,
headed by a line saying that a choice initializes the working directory: the
`jazyk.toml` and `.jazyk/` that [`jazyk init`](#jazyk-init) writes, without its
questions. `gui` needs no project and initializes nothing. The home directory and the
file system root are never initialized: a choice there says to run it from a project
folder and exits `1`.

The launched program inherits the terminal and its exit code is jazyk's. A program
that cannot be found is named with where to get it, exit `1`; the setup it would have
needed is still written, so the next start after installing it works straight away.
A setup step that fails (an unreadable config file, a command that exits non-zero)
names the file or the command and exits `1` without starting the program.

When stdin or stdout is not a terminal, bare `jazyk` prints the status (inside a
project) and the list with the command each choice runs, and exits `0`, so a script
never lands in a prompt. A choice command off a terminal never asks either: an IDE
choice takes every row that needs no consent and keeps the default driver.

### IDE agents

An IDE choice sets up the agents that work inside that IDE the first time, then
starts it. Each later start opens the IDE at once.

1. Setup check. Each agent's setup has a state: `installed` (the entry is there and
   current), `stale` (the entry is there with another command or arguments),
   `missing`, or `unknown` (a [guided step](./mcp.md#agent-setup), which jazyk cannot
   read back). When any row for this IDE is `installed` or `stale`, the IDE is
   already set up: stale entries are corrected without asking, the checklist is
   skipped, and the start follows.
2. Agents. Only when nothing is set up yet: a checklist of the Jazyk agent
   ([ACP](./acp.md)) and one row per coding agent jazyk detects for that IDE, each
   with the file its setup writes ([agent setup](./mcp.md#agent-setup)). A coding
   agent counts as detected when its program is on `PATH` or its extension or plugin
   is installed in that IDE. A detected agent jazyk has no setup for is listed dim
   as `unsupported` and cannot be ticked. Every row starts ticked: space toggles a
   row, `a` toggles all, enter confirms, and `q`, escape, or control-C start nothing.
   When only one row can be ticked it is taken without asking.
3. Driver. The Jazyk agent drives a coding agent of its own ([agents](./acp.md#agents)).
   When the Jazyk agent is set up and the project's `jazyk.toml` names none
   (`[acp] agent`), a list asks which: `embedded`, the built-in agents whose program
   is found, and the profiles the project or the global config defines, the default
   (`embedded`) under the cursor. With one option it is taken without asking. The
   answer lands in `[acp] agent`, so the question is asked once per project.
   `--agent NAME` answers it without the list, and records the name even when one is
   set. Later, `/agent <name>` in the IDE's Jazyk chat switches it
   ([switching agents](./acp.md#switching-agents)). A built-in profile shadowed by an
   `[acp.agents.<name>]` table is named with the file that defines it.
4. Start. The IDE starts on the root, and a short summary follows: that it started,
   where the Jazyk agent is reached in this IDE, the coding agents that have jazyk's
   tools, the text to paste for any guided step, a line per setup that failed with
   the reason, and the [`jazyk setup`](#jazyk-setup) command that changes the setup.
   Files written along the way are not listed one by one.

E.g.:

```
jazyk: started IntelliJ IDEA
  Jazyk agent: the AI Chat window, the agent selector at the bottom: Jazyk
  jazyk tools: Claude Code, Codex, JetBrains AI Assistant
  change with `jazyk setup idea`
```

### jazyk setup

`jazyk setup [ide] [--list] [--remove NAMES]` checks, adds, and removes
[agent setup](./mcp.md#agent-setup). With an IDE (any launcher IDE name, e.g.
`idea`) it covers that IDE's rows, as in [IDE agents](#ide-agents); without one, every
installed IDE and every detected agent, as in [`jazyk init`](#jazyk-init).

In a git repository, setup also registers the `jazyk` merge driver in the repository's
git configuration (`merge.jazyk.driver` set to `jazyk merge-file %O %A %B %P`), which
`.jazyk/.gitattributes` names for set files. Every build registers it too when it is
missing. See [merging through git](../compiler/jazyk-folder.md#merging-through-git).

- Bare, on a terminal: the checklist, each row ticked when its setup is `installed`
  or `stale` and labeled with its state. When nothing is set up yet every row starts
  ticked. Enter applies the difference: a ticked row that is `missing` or `stale` is
  set up, an unticked row that is `installed` or `stale` is removed. The result is one
  line each for what was added and what was removed.
- `--list`: one line per row with its state and its file, nothing written. Off a
  terminal, bare `jazyk setup` does the same.
- `--remove NAMES`: remove the named rows without the checklist. Names are the
  [agent setup names](./mcp.md#agent-setup), `jazyk` for the Jazyk agent (with an
  IDE; `jazyk-vscode`, `jazyk-cursor`, `jazyk-zed`, `jazyk-jetbrains` without one),
  `agents-md`, `trust`, or `all` (every agent and Jazyk agent row; `agents-md` and
  `trust` are only removed by name).
- `jazyk setup agents-md`: create or refresh the
  [jazyk section of `AGENTS.md`](./agent-context.md#the-jazyk-section-of-agentsmd)
  and the `CLAUDE.md` and `GEMINI.md` imports. `--remove agents-md` deletes them.
  Set `[agents] block = false` first to keep them removed.
- `jazyk setup trust`: list the project's MCP servers and skills and record them as
  [trusted](./agent-context.md#trust). `--remove trust` revokes the trust.
- `--list` also prints an `agents-md` line (`installed`, `missing`, `stale`, or
  `off`) and a `trust` line (`trusted`, `untrusted`, `changed`, or `none`).

Removing an entry deletes the `jazyk` server (or the `Jazyk` agent) from its file and
leaves everything else in the file as it was. A file left holding nothing else is
deleted, and so is a folder inside the project that this leaves empty. The Claude Code approval in `.claude/settings.local.json` goes with its
entry. The Jazyk agent's registration is global, so removing it removes it from that
IDE for every project; the ACP Client extension stays installed. A guided step is
removed by hand: the line names where. Exit `0` when the command did what it asked,
`1` when a file could not be read or written, `2` on a usage error.

### jazyk init

`jazyk init [--mcp LIST] [--acp LIST] [--agent NAME]` initializes the current directory as a project root:

- Writes an empty project file, `.jazyk/jazyk.toml` with `version = 1` and no
  [document set](../compiler/document-sets.md), and the generated `.jazyk/.gitignore`.
  When the file already exists there, it warns and leaves it unchanged. When discovery
  resolved to a parent project before the file was written, the note says so: the
  nearest `jazyk.toml` wins, so the new file takes over from here down. It creates no
  starter documents; adding a document set chooses what compiles.
- Writes the [jazyk section of `AGENTS.md`](./agent-context.md#the-jazyk-section-of-agentsmd),
  creating the file when it is missing, and the `@AGENTS.md` import into an existing
  `CLAUDE.md` or `GEMINI.md`.
- Offers [agent setup](./mcp.md#agent-setup) for everything installed: the same
  checklist as [IDE agents](#ide-agents), across every installed IDE. One row per
  detected coding agent (jazyk's tools over MCP), then one row per installed IDE that
  can host the Jazyk agent (its [ACP registration](./acp.md#registration)). Every row
  starts ticked; a row whose setup writes a file outside the project asks before
  writing. `--mcp` takes a comma-separated list of agent setup names, `all` (every
  detected agent), or `none`; `--acp` takes IDE client names, `all` (every installed
  IDE that hosts the Jazyk agent), or `none`. Either flag skips the checklist for its
  half. A non-interactive stdin skips the checklist, so scripts never hang.
- Asks which [agent](./acp.md#agents) does the AI work: the `embedded` one, or a
  built-in external agent (`claude`, `codex`, `opencode`). The name lands
  in the project's `[acp] agent`; a built-in agent needs no profile. `--agent NAME`
  (or `none`) skips the prompt, and a non-interactive stdin keeps the default.
- For the embedded agent, asks which model. Jazyk asks the configured endpoint what
  it serves and lists that, so the choice is between models that exist rather than
  remembered names; a blank answer keeps the resolved one, and a typed name is taken
  as given. The answer lands in `[llm] model`. An unanswering endpoint is reported,
  not fatal. Inside an IDE the same choice is available per session through
  [session config options](./acp.md#choosing-a-model).

The server entry's writes all go through claimed batches (`begin_goals`, the binding,
generation, and verification rows); raw writes outside a batch are a second entry
running `jazyk mcp graph --write` ([write tools](../compiler/tools.md#write-tools)),
added by hand when graph surgery is wanted. A `--mcp`, `--agent`, or `--acp` value
that names nothing jazyk knows is a usage error before any file is written. A name in
`--mcp` or `--acp` that is known but not detected is set up anyway: naming it is the
consent. Exit `0`
when something was set up, `1` when nothing was written, `2` on a usage error.

### jazyk compile

`jazyk compile [path...]` runs one [build](../compiler/compilation.md#a-build) per
selected [document set](../compiler/document-sets.md#cli-and-lsp): the set `--set`
names, else every set claiming one of the paths, else every dirty set. A path no set
claims is a usage error. The
[reconciler](../compiler/reconciler.md) derives the goal board from the documents, the
graph, the ledger, and the [change records](../compiler/graph.md#change-records); the
scheduler [batches](../compiler/reconciler.md#batching) ready goals; and
[sessions](../compiler/sessions.md) resolve them until the board
[converges](../compiler/compilation.md#convergence). Sessions run as
[ACP worker sessions](./acp.md#worker-sessions) against the configured
[agent](../compiler/project-settings.md#acp), one at a time (compilation is sequential;
the build lease enforces it); the agent process lives for the run. `--sessions N` runs
at most N sessions and stops with an honest `incomplete`, so a project advances one
batch at a time and any moment can be copied for a
[snippet](../benchmark/benchmark.md#snippets-from-a-real-project). `--sessions 0`
derives the board, parks every ready goal with the limit as the reason, and makes no
LLM call. A value that is not a whole number is a usage error, never an unlimited run.
[Executor overrides](../compiler/project-settings.md#executors) per goal kind or class
send some sessions to a different agent.

`--select <path>` runs a [selected build](../compiler/compilation.md#selected-builds):
only the goals in the selection run. The flag repeats. A path is a document or a
folder; `<path>:<first>-<last>` selects lines of a document (one-based, inclusive).
E.g.: `jazyk compile --select docs/orders.md --select docs/billing.md:12-40`.
Selected paths choose the set like positional paths. A selected path that is not a
document of the set, or a malformed range, is a usage error. The board summary
line names the selection (`compile (selection): 4 goals ...`), and goals outside it
stay open.

Terminal output, in order:

- The board summary first, once the documents are parsed and the board is derived: the
  goal count, the count per kind, and the blocked count. E.g.:

```
compile: 27 goals (12 reconcile-section, 9 rejudge-pair, 6 retrace), 3 blocked
```

- The live trace while sessions run (below).
- One `gc burst:` line as each [GC burst](../compiler/compilation.md#compile-and-garbage-collection)
  starts, naming the goal kind, the target, and the count against the
  [limit](../compiler/graph.md#limits) that opened it. E.g.:

```
gc burst: abstract-entity ent:order (54 > 50)
```

- The verdict last, with its counts. `converged` when no mandatory goal of either
  class is open or failed and the [checks](../compiler/compilation.md#checks) are
  clean; the blocked and optional counts ride beside it. `incomplete` otherwise, with
  the counts of open, failed, blocked, and optional goals. E.g.:

```
converged, 2 blocked, 1 optional advised
incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised
```

Live trace:

- Default: the session lifecycle lines (`batchStart`, `sessionStart`, `sessionDone`,
  `sessionFailed`), one line per session round showing the tool calls with condensed
  arguments, the condensed results, and the model's reasoning text, and one line per
  goal resolved (with its justification) or failed (with its reason). See
  [trace events](../compiler/sessions.md#trace-events). Endpoint trouble prints here
  too: admitted retries, rate-limit waits and capability errors are trace events,
  so every frontend sees them.
- `--verbose`: additionally prints every session's assembled
  [prompt](../compiler/sessions.md#the-prompt) (the goals block and the
  [loaded set](../compiler/context.md#the-loaded-set) as the model receives them),
  every `goal` event as it fires (opened with its cause, resolved with its
  justification: the cascade as it happens), the raw payloads, a line per model call
  for the request (messages, size) and its answer (elapsed, completion tokens), and the
  section each accepted call names.
- `--quiet`: prints the board summary, the `gc burst:` lines, and the verdict only.
- `--record`: writes a [graph snapshot](../compiler/sessions.md#snapshots) per batch to
  the log directory, for `jazyk replay extract`. Without it no snapshot is written.

Every build leaves a transcript: `compile`, `check`, `gen`, and `test` persist their
trace as one JSON-lines file under `<set-id>/trace/` in the
[log directory](../compiler/jazyk-folder.md#logs), the same format the
[GUI](./gui.md#jobs) writes and lists, so a CLI build shows up in the GUI's activity
panel. The metadata line carries `source: cli` and no job id. `--quiet` still
persists the transcript; only the terminal rendering is quiet. After the build,
[`jazyk ripple`](#jazyk-ripple) replays how every goal came to exist.

An immediately repeated `jazyk compile` prints `compile: 0 goals` and the verdict, and
makes no LLM call ([incremental builds](../compiler/compilation.md#incremental-builds)).

Exit codes: `0` on `converged` (the blocked and optional counts do not change it),
`1` on `incomplete` (an open or failed mandatory goal, parked work, or a failing
check), `2` on a usage error (see [help](#help)).

### jazyk check

Compile, then exit non-zero if open [diagnostics](../compiler/model.md#node-kinds) of
severity `error` exist, the findings of the [checks](../compiler/compilation.md#checks)
included. The CI gate. Exit `1` when the build ends `incomplete` or an open `error`
diagnostic remains after it, `0` otherwise.

### jazyk watch

Recompile on file change, using native file events (with a polling fallback when no
watcher is available). Event bursts debounce, and a fingerprint over the matched
documents decides whether a build actually runs, so editor temp files and the out
directory's own writes never trigger one. The same loop as `compile`: each change feeds
the [dirty set](../compiler/reconciler.md#dirty-set), so unchanged documents are not
revisited. See [incremental builds](../compiler/compilation.md#incremental-builds).

Terminal output per build is the board summary, then one line per goal, then the
verdict. A goal line says what opened (the kind, the target, the cause), what session
took it, and how it ended: resolved with its justification, failed with its reason, or
parked. `gc burst:` lines print as in `compile`. `--verbose` prints the full `compile`
trace instead. E.g.:

```
opened   g:reconcile-section:docs/orders.md#/orders/holds  (g87 via section-dirty)
taken    g:reconcile-section:docs/orders.md#/orders/holds  b87-1
resolved g:reconcile-section:docs/orders.md#/orders/holds  req:orders-6 revised (quote, statement, transition guard)
opened   g:rejudge-pair:req:orders-6~req:payment-9  (g88 via entities)
```

A build that ends `incomplete` (work parked, e.g. by a transient endpoint outage)
retries on its own with backoff (30s doubling to 5 minutes, reset by any file change)
instead of idling until the next edit. The verdict that decides it is the named set's
with `--set`, otherwise that of each set the watch compiles (the sets the paths claim,
or every set). [Parked goals](../compiler/reconciler.md#parked-and-failed) resume first. Unfinished
work is never silent, and watch is the loop that owns resuming it.

### jazyk monitor

`jazyk monitor [--json] [--once]` watches the same surfaces `watch` does but performs
nothing: on every state change it prints the ready goals on the
[board](../compiler/reconciler.md#readiness) and which MCP tool claims them, then goes
quiet until the next change. One block per notice; `--json` prints one JSON object per
line instead. Output is flushed per notice, so a pipe reads events as they happen.
Blocked goals print with their reason (an unanswered prompt, a ratification proposal, a
gated release), so the notice says what a human owes. The text notice lists at most
five ready goals and three blocked ones, the goals blocked on a human before the gated
ones, and counts the rest (`... and N more ready`, `... and N more blocked`); the JSON
notice carries every goal. E.g.:

```
jazyk: 3 goals ready, 1 blocked
  reconcile-section docs/template.md#/template/usage (section dirty, 1 stale anchor)
  reconcile-section docs/template.md#/template/install (section dirty)
  retrace view:usecase/holds (member req:orders-6 gone)
  blocked: answer diag:decision-2 (awaiting a human answer)
  → call goals on the jazyk MCP server, then begin_goals to claim a batch
```

This is the external agent's trigger, in two shapes:

- Stream: the agent runs `jazyk monitor` under its own process monitor (any harness
  facility that makes a background command's output lines into events) and acts on
  each notice through [MCP](./mcp.md#the-work-loop). The process runs until killed.
- One shot: `--once` blocks silently until the board holds a ready goal, prints that
  one notice, and exits 0. Built for "wake me when there is something to do": a
  background shell awaiting the exit, or a script chaining
  `jazyk monitor --once && <act>`.

`watch` is the same trigger wired to the internal loop instead; the notice an agent
reads is the goal batch the internal loop would run.

In `manual` [mode](../compiler/control-plane.md#modes-and-releases), gated goals print
as "awaiting release" instead of prompting the agent to begin, and the notice fires
again when the release lands. `--once` waits for a goal that is actually claimable,
so a released click is what makes it exit.

### jazyk release

`jazyk release [compile|generate]` records a
[release](../compiler/control-plane.md#modes-and-releases): approve the pending changes
for the named stage (both when unnamed) without running anything. The watchers wake,
whichever worker is attached does the work. This is the scriptable form of the GUI's
release button. An explicit `jazyk compile` or `jazyk gen` is itself a release for
its stage: a typed command is an approval. The generate stage covers
[binding](../consumers/bind.md#when-binding-runs) too, and it journals any pending
document edit before approving the generation, so an edit saved since the last build
does not re-gate the release one command later; decompilation releases through
[`jazyk decompile`](#jazyk-decompile), never through `release`.

The output says what the release did: how many gated goals it approved, how many
goals are ready now per stage (compile, bind, generate), and a `next:` line naming the
command that runs them. In `auto` mode nothing is gated and the line says so. E.g.:

```
jazyk: released generate: 73 gated goal(s) approved; ready now: 63 compile, 0 bind, 0 generate
next: `jazyk compile` runs the compile goals (or an attached worker claims them)
```

Exit `0`; `2` when the stage is not `compile` or `generate`.

### jazyk answer

`jazyk answer <diagnostic|goal> [--option N | --text "..."]` answers one prompted
diagnostic from the terminal: the scriptable form of the LSP code action, the GUI
questions panel, and chat's `answer_diagnostic`
([answers](../compiler/model/diagnostic.md#answers)). The argument is a diagnostic id
(`diag:ratification-pending-6`), a `ratify` goal (`g:ratify:ent:funds`, resolved to
its proposal), or an `answer` goal (`g:answer:diag:decision-2`).

- Without `--option` or `--text`: print the question, its options numbered from 0
  (`edit` options with their document and replacement text, `answer` options with
  their label), and whether freeform is accepted, then exit 1. Nothing is written.
  Both together, or an `--option` that is not a whole number, is a usage error.
- `--option N`: choose one option. An `edit` option applies as a dual write, with the
  fact changes its rewrite implies
  ([answers](../compiler/model/diagnostic.md#answers)), and resolves the diagnostic in
  one changeset, printing each fact it changed and each transition it left to review;
  on a ratification proposal it is the
  accept ([the human path](../compiler/goals/ratify.md#the-human-path)) and the fact's
  provenance flips to `quote`. The retract option of a ratification proposal is
  deterministic too. Any other option records `handling` and runs an
  [answer session](./acp.md#answer-sessions) in the foreground, printing its outcome.
- `--text "..."`: instructions for the agent, including on a ratification proposal.
  It records `handling` and runs the answer session. It never inserts the reply
  directly into a document.

E.g.:

```
$ jazyk answer g:ratify:ent:funds
diag:ratification-pending-6  ratification-pending on ent:funds
  Should docs/checkout.md state it? "Funds: Funds gathers ..."
  0  Insert into docs/checkout.md /checkout (edit)
  1  Retract (answer)
  freeform accepted: --text "..."
$ jazyk answer g:ratify:ent:funds --option 0
jazyk: applied: the sentence landed in docs/checkout.md and the fact is quoted from it
```

Exit codes: `0` when the answer landed (or the answer session handled it), `1` when
nothing was written (no reply given, an unknown or already answered diagnostic, a
refused retract with its reason, a failed answer session), `2` on a usage error.

### jazyk status

`jazyk status [--set ID] [--json] [--costs]` reads only and takes no arguments. Its
last line names the project's [log directory](../compiler/jazyk-folder.md#logs).

Without `--set` it is the aggregate read over every
[document set](../compiler/document-sets.md#cli-and-lsp): one line per set with its
name, its state (`clean`, `dirty`, or `empty`), the generation, the last verdict
(`none` before the first build), and the board counts (open, ready, blocked, parked,
failed), then one indented `warning:` line per selection that matches no file. A
project without sets says so and names `jazyk sets add`. `--json` prints the
[sets summary](../compiler/document-sets.md#cli-and-lsp) instead, the same object
`jazyk sets list` prints. E.g.:

```
Documents: clean, generation 32, verdict converged; 0 open, 0 ready, 33 blocked, 0 parked, 0 failed
Orders: dirty, generation 4, verdict none; 12 open, 12 ready, 0 blocked, 0 parked, 0 failed
  warning: No files match docs/orders/**/*.md
```

A set whose store is newer than this Jazyk is one line with its `store-newer` refusal
instead (its state is `store-newer` in the sets summary, with the refusal in `error`),
and `jazyk status` exits with 1 ([store version](../compiler/graph.md#store-version)).
With `--set` on such a set it prints that line alone and exits with 1.

With `--set` it summarizes that set's `store.yaml` and `local/build.yaml` (see
[storage layout](../compiler/graph.md#storage-layout)) and the board. Outside a project
(no `jazyk.toml` above the working directory) it says so on stderr and reads the ad hoc
store at the working directory. The lines, in order:

- the store `version` and the generation counter,
- the last verdict with its counts,
- the board counts: open goals by class (compile, GC), blocked, parked,
  failed, optional advised, and the pairs waiting under the degree cap when any do
  ([pairs](../compiler/reconciler.md#pairs)). The board is derived from disk the same
  way `compile` derives it, so on a tree with pending edits `status` shows the goals
  the next build will run,
- [coverage](../compiler/compilation.md#coverage) percentage,
- open diagnostics by severity, ratification proposals excluded,
- the ratification proposals awaiting an answer, as one count, when any stand
  ([ratification proposals](../compiler/model/diagnostic.md#ratification-proposals)),
- the medium warning, when the ledger's medium and the recorded run commands disagree
  ([the medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)),
- the shape line: the entity count per depth of the containment tree (the
  [scope root](../compiler/concepts/levels.md#the-scope-root) at depth 0, its
  parentless entities at depth 1, and so on), then the fan-out histogram: how many
  nodes with children (the scope root included) hold how many direct children, in
  bands against the `children-per-entity` limit
  ([limits](../compiler/graph.md#limits)): at or under soft, over soft and at or
  under hard, over hard. A node over hard is what the `level-shape` check
  ([checks](../compiler/compilation.md#checks)) reports and what a mandatory
  `abstract-entity` goal on the board line is open for,
- the last build's cost (`costs`: sessions, tokens, the share per goal kind),
- the [unattached remainder](../consumers/gen.md#the-unattached-remainder): generated
  mass no requirement claims, summed over the ledger, with the worst entity's ratio,
- the [unclaimed report](../consumers/bind.md#the-unclaimed-report): deliverable
  files no binding names,
- `next:` lines, one per owed action, naming the command that moves the board: the
  first build when none has run, the goals blocked on a human with the `answer`
  command for the first of them, the ready goals with the command that runs them
  (`compile` for graph goals, `gen` and `test` for ledger goals), the gated goals
  with the `release` that approves them, the failed goals with the `explain` that
  shows the reason. With nothing owed the line says the graph reflects the docs, or
  points at `explain` when open goals all wait on something.

E.g.:

```
store: version 2, generation 413
verdict: converged, 2 blocked, 1 optional advised
board: 0 open (0 compile, 0 gc), 2 blocked, 0 parked, 0 failed, 1 optional
coverage: 96% (185 of 193 sections)
diagnostics: 1 error, 4 warnings
proposals: 12 await ratification (`jazyk answer` lists them)
shape: 3 / 11 / 42 nodes per depth; fan-out 2-9: 12, 10-15: 2, over 15: 0
cost: 41 sessions, 310k tokens (78% reconcile-section)
unattached: 3 file(s), 120 line(s) (worst cart at 0.32)
unclaimed: 3 file(s) no binding names (`jazyk decompile` drafts docs for them)
  - src/cart.rs
  - src/coupon.rs
  - src/tax.rs
next: 2 goal(s) wait on a human: `jazyk answer g:ratify:ent:funds` lists the options
```

`--json` prints one JSON object with the same fields (`root`, `out`, `version`,
`generation`, `verdict`, `board` with its counts including `ready` and `gated`,
`coverage`, `diagnostics`, `medium`, `shape`, `cost`, `unattached`, `unclaimed`,
`next`) for scripts. Exit `0`.

`jazyk status --costs` prints the cost report instead of the status lines: what the
whole journal spent, so a change to a contract or a skill can be judged on cost as well
as on quality. It reads the [journal](../compiler/graph.md#journal) and the graph and
nothing else: no board, no LLM call, no write. The journal spans every build since the
store began (an [archive](../compiler/graph.md#store-version) starts it over), so the
report covers every build, where `costs` in `build.yaml` covers the last one.

- A `session` entry's `tokens` is its measured spend: the tokens the endpoint billed,
  input and output of every round ([budgets](../compiler/sessions.md#budgets)), so the
  per goal, per requirement, and per criterion figures are costs, not output alone. The
  spend splits evenly across the goals the entry resolved (the goals of its `batch`
  when it resolved none).
- `cost`: the journal's generation range, the session entries (measured, and
  agent-reported when any entry carries `tokensSource: agent`, see
  [spend](#jazyk-spend), and unmeasured), the tokens of both, and, when any entry
  carries `cachedTokens`, the
  cached input tokens with their share of the spend
  ([budgets](../compiler/sessions.md#budgets)). An entry without `cachedTokens`
  contributes no cached tokens: older entries and endpoints that report no cache read
  count as 0 cached, never as unmeasured.
- `by kind`: per goal kind, the goals resolved, their tokens, their cached tokens,
  tokens per measured goal, and the goals whose session was unmeasured. Heaviest kind
  first.
- `by document`: per document, the goals that target it, their tokens, their cached
  tokens, and the unmeasured goals. A goal's document is its target's:
  - a section reference or a document path names it,
  - a node's is the document its quote sits in: an entity without a quote of its own
    takes its first mention's, a criterion without one takes its requirement's, a
    merged node takes its survivor's,
  - a pair's share splits evenly across its two sides, and the goal counts under each
    side's document,
  - a target with no document (a view, a scope, a derived node, a node since deleted)
    lands under `(no document)`.
- `per requirement` and `per criterion`: the measured tokens divided by the
  requirements and the criteria in the graph now.

A `session` entry without `tokens` (an external agent drove it, see the
[journal](../compiler/graph.md#journal)) is unmeasured: it counts toward the sessions,
the goals, and the `unmeasured` columns, never as zero tokens. The per goal, per
requirement, and per criterion figures cover measured spend only, and a journal with no
measured session says its spend is unknown instead of printing them.

E.g.:

```
cost: g1..g43, 38 sessions (36 measured, 2 unmeasured), 164210 tokens, 61204 cached (37%)
by kind:               goals   tokens   cached  per goal  unmeasured
  reconcile-section       31    61204    24910      2040           1
  derive-criteria         16    45611    18102      2851           0
  review-entity           19    27830     9877      1546           0
by document:           goals   tokens   cached  unmeasured
  docs/orders.md          14    21870     8104           0
  docs/payment.md         11    18342     7011           1
  (no document)            3     1904        0           0
per requirement: 1954 tokens (84 requirements)
per criterion: 1253 tokens (131 criteria)
```

The `cached` figure and its share print only when some entry carries `cachedTokens`;
a journal without one prints the line and the tables as before, without the cached
column.

A `session` entry the agent priced itself (`tokensSource: agent`) counts in the tokens
like a measured one, and apart from it in the session counts: the line reads
`38 sessions (35 measured, 1 agent-reported, 2 unmeasured)`, the agent-reported count
printed only when there is one.

`--costs --json` prints one JSON object with the same content: `first`, `last`,
`sessions`, `measured`, `reported` (the agent-reported sessions), `unmeasured`,
`tokens`, `cached` (the cached input tokens
over the measured sessions, `0` when none reported any), `by_kind` and `by_document`
(each a list of `{name, goals, tokens, cached, unmeasured}`, heaviest first),
`requirements`, `criteria`, `per_requirement`, `per_criterion`. A figure no session
measured is `null` (and `-` in the text), never `0`. Exit `0`.

### jazyk version

`jazyk version [--json]` prints this build's [version](./distribution.md#versions), its
commit, the [store version](../compiler/graph.md#store-version) it reads and writes,
and the [editor protocol](./protocol.md). `jazyk --version` (or `-V`) prints the
version line alone, e.g. `jazyk 0.2.2+31.ge0fff82`. Both read nothing from the project
and exit with 0. E.g.:

```
jazyk 0.2.2+31.ge0fff82 (unreleased, 31 commits after 0.2.2)
commit e0fff82d8252b0357681ea22f20bf0f50894448c
store version 5
protocol 2
```

A release prints `jazyk 0.2.2` on the first line. `--json` prints one object:
`version`, `release` (the last release), `released`, `commitsAfter` (`null` when
unknown), `commit` (empty when unknown), `store`, and `protocol`.

### jazyk spend

`jazyk spend <generation> <tokens> [--cached N]` records what an external agent's
session billed on its journal entry after the fact. A session an external agent drove
over MCP carries no `tokens` unless the agent called
[`report_spend`](../compiler/tools.md#spend-tool) during the batch; a driver that learns
its usage only after the run (Codex prints per-turn usage in its JSON output) stamps it
here.

- `<generation>`: the `session` entry to stamp, `g42` or `42`.
- `<tokens>`: the tokens the agent's endpoint billed for that session, input and output
  of every round. A whole number above zero.
- `--cached N`: the input tokens among them the endpoint served from its prompt cache.
  At most `<tokens>`.

The entry takes `tokens`, `cachedTokens` when given, and `tokensSource: agent`
([journal](../compiler/graph.md#journal)), so `jazyk status --costs` counts the session
as agent-reported, apart from measured spend. The command refuses an entry that already
carries a figure, measured or reported, and says what it carries: a stamp never
overwrites what the harness measured, and a driver that reports twice is told so. It
refuses a generation that is not a `session` entry, and one the journal does not hold.
It reads the one journal file and writes it back: no board, no LLM call, no other write.

E.g.:

```
$ jazyk spend g42 18400 --cached 9100
g42: 18400 tokens, 9100 cached, reported by the agent
```

Exit codes: `0`; `1` when the entry is missing, is not a session, or already carries a
figure; `2` on a bad argument.

### jazyk preview

`jazyk preview [goal|target|batch]` renders the next session's prompt exactly as the
model would receive it: the agent contract, the active skills, the project block, the
goals block for the batch, the loaded set with its handles, and the worker protocol line
(see [the prompt](../compiler/sessions.md#the-prompt)). With a goal id, it renders the
batch that goal would join; with a target (a node id, a section reference, a document
path, whose goals sit on its sections), the batch of the first ready goal on that
target; with a batch id (`b<generation>-<n>`, as the
[board](../compiler/reconciler.md#batching) lists them), that batch's prompt; without
an argument, the batch the scheduler would claim next. The prompt is the one the next
session receives, batch id included: the protocol line names the id the board minted
for that batch, the same id `begin_goals` accepts, and the project block carries the
control plane's mode, never a constant. A goal that is not ready yet renders all the
same, with a `not ready:` line and the readiness reason above the prompt, so its
session can be inspected before its tier arrives; since the scheduler has not formed
its batch, the protocol line carries a placeholder id (`b<generation>-?`) and the
`not ready:` line says so. The rendering is deterministic, so what `preview` prints is
what the session will spend; it makes no LLM call and writes nothing. The
[GUI](./gui.md) shows the same pane before a release in `manual` mode
([preview](../compiler/sessions.md#preview)).

`ratify` and `answer` goals have no session; for those `preview` prints what the human
owes instead of a prompt, closing with the `jazyk answer` command that lists the
options.

Exit codes: `0` when a prompt was rendered, `1` when nothing rendered (an empty board,
an unknown goal, a target with no goal, a blocked-on-human goal); the reason prints.

### jazyk explain

`jazyk explain [goal|target]` is a rendering over derivable state: the board, the
change records, and the graph. It makes no LLM call and writes nothing.

- For a goal: the change record that produced it (kind, subject, `via`, the detail),
  its cause (generation, mutation, the edge or computation), its class and whether it
  is mandatory, its [readiness](../compiler/reconciler.md#readiness) (the tier, ready
  or the reason it waits, rendered as a sentence), what blocks it, its hints, and for
  a [parked](../compiler/reconciler.md#parked-and-failed) goal a `parked:` line with
  the reason the build parked it (e.g. `parked: the build cap of 23 sessions ran
  out`). E.g.:

```
g:retrace:view:usecase/holds  retrace, compile, mandatory, open
  change: view-member-gone req:orders-6 (deleted in g409, reason: duplicate)
  cause:  g409 mutation 2 via members
  ready:  tier 2; waits for g:reconcile-section:docs/orders.md#/orders/holds (tier 1, same cone)
  hints:  load view:usecase/holds; skill flow-views
```

- For a pair waiting under the degree cap ([pairs](../compiler/reconciler.md#pairs)),
  named by its goal id or its target: that it waits, its score, which side holds
  `pairs-per-requirement` open pairs, and the records it stands on. `preview` on such
  a pair prints the same line and no prompt, since no session runs for it yet.
- For a target (a node id, a section reference, a document path): the cone of goals a
  change to it would open, walking the stored references and the computed derivations
  from it, each with the kind, the target, and the edge it is reached through, plus
  the [derived data](../compiler/graph.md#derived-data) a commit would recompute. E.g.:

```
ent:order: a change here opens
  review-entity ent:order              via entity-changed
  conform-instance ent:order-4711      via instantiation
  retrace view:usecase/holds           via members
  retrace view:sequence/holds          via members
  recomputed at commit: rel:order~order-item, sm:order, view:class/commerce
```

- Without an argument: the whole board, one line per goal (id, class, mandatory or
  optional, state, readiness), ready goals first, then waiting, blocked, parked,
  failed.

Exit codes: `0`; `1` when the goal or target is unknown.

### jazyk ripple

`jazyk ripple [target|generation|doc] [--back]` prints the ripple DAG rooted at a
change: every generation the root led to, the goals each generation opened and the
sessions that resolved them, each with its cause and its one-line justification. It is
a rendering over the [journal](../compiler/graph.md#journal); nothing is stored for it.
Without a root it walks the last build: the cascade rooted at the generation the build
opened with, the whole-build report.

- For a target (a node id): the last cascade that touched it, from the human edit at
  its root through the generations that followed. A cascade is rooted at an entry that
  carries goals (a session, an edit, a decree); an entry that only records a check run
  names the node without explaining it, so it is skipped when choosing the root. Rooting
  there printed one line and no chain for a requirement a whole build had worked on.
- For a generation (`g87`, or `87`): the full tree forward from it. Rooted at the
  generation a build opened with, this is the whole-build report: the causality DAG,
  the cost totals (sessions, tokens, per goal kind), and the parked and failed goals
  with their reasons (`parked: <goal> (<reason>)`, `failed: <goal> (<reason>)`). A
  generation doubles as the whole-build report this way.
- For a document path: the cascade rooted at the last `edit` entry that dirtied it.
- `--back`: causes instead of consequences. From the root, walk backward through the
  `updated` markers and the recorded causes to the human edit that started everything.

The trace a one-sentence edit leaves (`orders.md`: "held orders expire after 21 days"
becomes "30 days"):

```
edit g87 docs/orders.md#/orders/holds (human)
└─ reconcile-section docs/orders.md#/orders/holds g88: req:orders-6 revised (quote, statement, transition guard)
   │  recomputed at commit: sm:order (held→expired guard), view:sequence/holds
   ├─ rejudge-pair req:orders-6~req:payment-9 g89: consistent
   └─ bind req:orders-6 g90: row stale (requirement-changed), test rewritten, no file implements 30 days → unimplemented
      └─ generate ent:order g91: files rewritten
         └─ verify req:orders-6 g92: pass
gc: no goals derived
converged: 4 sessions, 2 recomputes, 29k tokens
```

A session that resolved one goal prints as one line, as above. A batch that resolved
several prints a header naming the kinds and the count, then one line per goal with its
target and its justification, indented under it:

```
├─ rejudge-pair ×8 g44
│    req:expenses-10~req:expenses-11: consistent, one states the deadline and the other the handoff
│    req:expenses-11~req:expenses-17: consistent, different steps of the same claim
```

Joining eight goals and their justifications into one line, as this once did, put the
whole batch past the width of any terminal.

Every line is a journal entry; every indent is a goal with its cause and justification
on record. The `recomputed at commit` line is
[derived data](../compiler/graph.md#derived-data) following the requirement without a
session. Parked and failed goals print after the tree with their reasons; a build with
none says so.

Exit codes: `0`; `1` when the root is unknown (no journal entry touched the target, no
such generation, no `edit` entry for the document) or the journal is empty.

### jazyk context

`jazyk context <target> [--depth N] [--expand HANDLE...]` prints what the
[`load`](../compiler/context.md#tools) tool renders for a target: the target in full,
its edges, each neighbor as a stub, and the [status block](../compiler/context.md#rendering)
of the resulting [loaded set](../compiler/context.md#the-loaded-set) with its
[handles](../compiler/context.md#policy). This is the debug window into the loaded set:
what this command prints is exactly what a session sees after `load({target, depth})`,
under the same context budget (a registry constant, see
[the limits registry](../compiler/graph.md#limits)). `--expand` follows the named handles before
printing, as `expand` would.

`<target>` is any node id (entity, requirement, view) or a section reference. E.g.:

```
jazyk context ent:shopping-cart --depth 2
jazyk context view:class/commerce --expand h:view:class/commerce:members
```

### jazyk query

`jazyk query <text> [--json]` runs the [search tool](../compiler/tools.md#read-tools)
and prints the matches, one `id (name): definition` line each; `--json` prints one
`{"id", "name", "definition"}` object per line instead. Exit `0` when something
matched, `1` when nothing did (the text form says so on stderr).

### jazyk gen

`jazyk gen [entity...]` runs the built-in [generation](../consumers/gen.md) worker over
the ledger goals: [`bind`](../compiler/goals/bind.md), [`generate`](../compiler/goals/generate.md),
and [`verify`](../compiler/goals/verify.md) sit on the same board at tier 3. `bind` and
`generate` are gated by the generate
[release](../compiler/control-plane.md#modes-and-releases), and `jazyk gen` is that
release. `verify` is not release-gated (verification writes nothing into the
deliverable). It runs `generate` goals without an exhaustive binding phase first:
each entity's source and tests develop together in one bounded session. The coding
agent chooses their order and reuses existing bindings. Implementation is written into the
[deliverable](../compiler/project-settings.md#generation), within the set's `source`, and recorded as a
manifest in the [ledger](../consumers/gen.md#the-ledger). With no arguments it covers
every entity that has at least one requirement, leaf entities first, grouped by
component where containment gives one, skipping entities whose facts are unchanged
(`--force` regenerates everything). Dense entities generate in parts. The layout and
run commands are the generator's choices, derived from the documents; the
[medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
is decided once per deliverable, before the first session. When that medium must be
built, the run ends by running [the build](../consumers/gen.md#the-build) and says what
it produced, or what it said when it failed. A choice the documents do not settle is
recorded as an `invented-choice` diagnostic graded by the scope of the invention.
`jazyk codegen` and `jazyk testgen` print a pointer to `jazyk gen` and exit `2`
without running anything.

### jazyk test

`jazyk test [target...]` runs [verification](../consumers/gen.md#runners): the
[`verify`](../compiler/goals/verify.md) goals. With no arguments it processes every
runnable ledger row; entity ids select their requirements' rows; requirement ids select
rows directly. `programmatic` rows execute their recorded command (exit 0 is a pass);
`llm` rows run the in-process harness, judging the criteria against the requirement's
`statement`. `--kind programmatic|llm` filters; `--force` also reruns `verified` rows;
`--list` prints the derived status table (one row per requirement: id, `statement`,
status, and for a row that is not `verified` the reason and the repair the ledger
names, e.g. `stale-test  the test file changed  rerun: jazyk test req:orders-6`)
without running; `--audit` rebuilds the ledger from the artifact markers. Exit
0 when every targeted row is `verified`, 1 otherwise.

### jazyk decompile

`jazyk decompile [path...]` runs [decompilation](../consumers/decompile.md): draft
documents describing what the code under the named scopes does (default: the whole
[unclaimed report](../consumers/bind.md#the-unclaimed-report)). The command records
the decompile release for its scopes; with an agent attached and preferred by the
[dispatch](../compiler/control-plane.md#dispatch) preference, the agent's watcher does
the drafting, otherwise the built-in worker runs each scope as one draft session with
read-only file tools over the deliverable. Drafts land in the docs tree carrying
`unratified` diagnostics until edited ([ratification](../consumers/decompile.md#ratification)).
Decompilation is not a board goal and has no auto mode; this command and the GUI's
decompile action are the only triggers.

### jazyk docsgen

`jazyk docsgen` renders the documentation pages into `<out>/derived/docsgen/` on demand, without
compiling: the per-entity requirements documents (each entity's definition, its
requirements with their quotes, its relationships, and the rendered images of its
relevant views), the [entity cards](../consumers/docsgen.md#entity-cards) under
`entities/`, the [level pages](../consumers/docsgen.md#level-pages) under `levels/`,
and the [diagram pages](../consumers/docsgen.md#diagram-pages) under
`diagrams/<kind>/`, every image linked relatively into `<out>/derived/diagrams/`
([output layout](../compiler/diagrams.md#output-layout)). The summary line counts every
page written, all four kinds together. The [diagrams](../compiler/diagrams.md#rendering)
render with it, skipped for unchanged `.puml` content, so every image link resolves.
The same render runs after every committed changeset. See
[documentation generation](../consumers/docsgen.md#the-requirements-document).

### jazyk viewer

`jazyk viewer [--out FILE]` renders the graph into one self-contained HTML file
(default `<out>/derived/graph.html`). See [viewer](./viewer.md).

### jazyk gui

`jazyk gui [--port N] [--no-open] [--watch] [--gui-url URL] [--no-token] [--dev]` starts the
[GUI](./gui.md)'s server: the JSON API, the event stream, and the language server over
WebSocket, then opens the editor page at [jazyk.dev](../site.md#the-editor) with the
port and the token in the address, so the page connects to this server
([connecting to a local jazyk](./gui.md#connecting-to-a-local-jazyk)). The binary
carries no page. `--no-open` prints the address instead of opening the browser.
`--gui-url URL` (or `JAZYK_GUI_URL`) opens another copy of the page (a dev server, a
fork) and admits that origin. `--watch` starts in the automatic
[compile mode](./gui.md#workflow-modes) (default:
changes queue and compiling is an explicit click). Binds `127.0.0.1` only.

### jazyk mcp graph

Start the [MCP server](./mcp.md) on stdio. Read tools by default; `--write` adds the
[write tools](../compiler/tools.md#write-tools). `jazyk mcp <toolsets>` serves the
other [toolsets](./mcp.md#toolsets): `compile` claims goal batches
([compilation over MCP](./mcp.md#compilation-over-mcp)), `generate`, `verify`,
`decompile`, `benchmark`, `chat`; `all` is `compile,generate,verify,decompile,chat`,
the entry `jazyk init` writes.

### jazyk acp

`jazyk acp` starts the [IDE-facing ACP proxy](./acp.md#the-ide-proxy) on stdio. An
IDE spawns it as its "Jazyk" agent; it drives the configured downstream agent and
adds jazyk in between: tool injection, doc edit delegation, slash commands, build
status. Outside a jazyk project it is a transparent passthrough. Not meant to be run
by hand.

`jazyk acp install --ide <client>` registers Jazyk with an ACP client:
`zed`, `jetbrains`, `vscode`, `cursor`, `neovim`, `emacs`, `obsidian`, `acpx`, `marimo` (see
[registration](./acp.md#registration)). The editor may also be given positionally
(`jazyk acp install zed`). For a client whose config jazyk writes, the entry is
merged in place, leaving comments and other agents alone; for the rest, the snippet
to paste is printed. Exit `0` when the entry was written, was already current, or the
snippet was printed, `1` when a file could not be written.

### jazyk agent

`jazyk agent` starts the [embedded ACP agent](./acp.md#the-embedded-agent) on stdio:
a generic agent over the configured [LLM endpoint](../compiler/project-settings.md#llm)
with no jazyk knowledge. Jazyk spawns it when the `embedded` profile is selected. Not
meant to be run by hand.

### jazyk lsp

`jazyk lsp [--stdio]` starts the [language server](./lsp.md) on stdio. It serves the
last committed graph and its rendered diagrams, and a rebuild refreshes it. It writes
only through its explicit [commands](./lsp.md#commands): answering a diagnostic,
editing a document set, and `jazyk.compile`, which builds one set with the LLM
settings this command resolved. `--stdio` is accepted and changes nothing: LSP clients append it
to name the transport (the VS Code client library does), and a server that refused it
would crash on every start.

### jazyk merge-file

`jazyk merge-file <base> <ours> <theirs> <path>` is the git merge driver for set files,
run by git, not by hand. It merges one committed file of a set field by field and
writes the result over `<ours>`, exiting zero, or leaves conflict markers and exits
non-zero when the file does not parse. The rules are in
[merging through git](../compiler/jazyk-folder.md#merging-through-git).

### jazyk replay

`jazyk replay [file|dir...]` runs [replays](../benchmark/replays.md): cases whose tool
calls are scripted, graded against the harness without a model. It prints one line
per step and per check and exits non-zero on any mismatch. With no argument it runs
the replays under `docs/benchmark/replays/`. `jazyk replay extract <trace.jsonl>
<label>` writes a replay for one session of a trace to stdout, the recorded outcomes
as its expectations, for the author to trim and correct.

### jazyk benchmark

Grade whether the configured [agent](../compiler/project-settings.md#acp) and model
can drive jazyk's [sessions](../compiler/sessions.md). See
[benchmark](../benchmark/benchmark.md); cases per goal kind are deferred there. Results
land in `<set-id>/benchmark/results.yaml` in the
[log directory](../compiler/jazyk-folder.md#logs), per-case transcripts under
`<set-id>/benchmark/trace/`. `jazyk benchmark [case...]` grades
[a subset](../benchmark/benchmark.md#running-a-subset) for iterating on one failure.
`jazyk benchmark --project <dir> --goal <goal-id>` runs one goal's session from a
copy of a real project, prints the outcome and the transcript path, and touches
nothing ([snippets](../benchmark/benchmark.md#snippets-from-a-real-project));
`--force` runs the goal even when the board holds it blocked.

## Common options

- `--agent NAME`: the [ACP agent profile](../compiler/project-settings.md#acp) to use.
- `--llm-base-url URL`: the LLM endpoint (used by the embedded agent).
- `--model M`: the model to use (used by the embedded agent).
- `--api-key K`: sent as a bearer token (used by the embedded agent).
- `--out DIR`: the out directory (default `.jazyk/<set-id>/`). It requires `--set` and
  never names project metadata or another set's store.

## Help

- `jazyk --help` (or `-h`, or `jazyk help`) prints the top-level usage: one line per
  command, in four groups in this order, plus the common options. The groups put what
  a person types most at the top and what programs spawn at the bottom:
  - Start here: `init`, `setup`, `gui`, `sets`, `status`, `compile`, `benchmark`, and
    one line naming the [launcher's](#jazyk) choices, each a command.
  - Actions on the project: `check`, `watch`, `gen`, `test`, `decompile`, `docsgen`,
    `release`, `answer`, `spend`, `replay`, and the task commands `tasks`, `runs`,
    `run`, `run-status`, `run-log`, `run-cancel`.
  - Inspect: `preview`, `explain`, `ripple`, `context`, `query`, `viewer`.
  - Run by IDEs and agents, not by hand: `mcp`, `acp`, `lsp`, `agent`, `monitor`.
- `jazyk <command> --help` prints that command's usage: its arguments, the options it
  honors (only those), and its exit codes where they carry meaning. `jazyk help <command>`
  is equivalent.
- Help prints to stdout and exits `0`. Bare `jazyk` is the [launcher](#jazyk), never
  the help; an unknown command (with or without `--help`, or after `help`) prints one
  line naming it and pointing at `jazyk --help`, and exits `2`.
- The options a command honors are exactly the ones its help lists. An option a
  command does not honor, an option jazyk does not define, an option missing its value,
  a value that is not a whole number where one is required (`--sessions`, `--depth`,
  `--port`, `--option`), or more positional arguments than the command takes is a
  usage error: one line naming the problem and the help to read, on stderr, exit `2`.
  Nothing runs and nothing is written. A misspelled option is never taken for a
  document path, and a bad `--sessions` value is never taken for an unlimited run.

## Project discovery

The CLI walks up from the working directory to the nearest `jazyk.toml` and treats that
directory as the project root. A [redirect](../compiler/project-settings.md#redirect)
found above the working directory is a boundary, not a capture: discovery stops there
and the command runs ad hoc at the working directory. Paths never skip discovery: the
paths `jazyk compile` takes select the document sets that claim them, and `check` and
`watch` take no paths. Each set's state lives in `<root>/.jazyk/<set-id>/`.

## Project task commands

`jazyk tasks` lists task definitions and runtime availability. `jazyk runs` includes
the recent run history. `jazyk run <task-id>` submits a finite task and returns its
durable run ID. `jazyk run-status <run-id>`, `jazyk run-log <run-id>`, and
`jazyk run-cancel <run-id>` inspect, read, and cancel it. They print JSON and follow
the [help](#help) rules: `--help` prints their usage, and a missing id, an extra
argument, or an unknown option is a usage error with exit `2`. These commands use local
Docker without a Cloud account. See [Task execution](../compiler/task-runtime.md).
