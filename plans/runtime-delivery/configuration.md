# One list of tasks

Status: revised proposal, 2026-10-15. This replaces the earlier separate runtime,
cache, task, test, and delivery tables. Syntax is illustrative, not implemented.

## The complete basic model

A project has zero or more tasks. A task has a stable `id`, a human `name`, a
`description` explaining when to use it, and a shell `command`. A run is one
execution of that task against a frozen project snapshot.

Build, test, lint, render, publish, and delivery describe what a task does. They are
not different configuration types. The model reads the names and descriptions and
chooses the useful task. People choose from the same list.

E.g.:

```toml
[[tasks]]
id = "install"
name = "Build and install"
description = "Build the Maven project and install it into the local dependency repository."
command = "mvn install"
products = ["target/*.jar"]
cache = [".cache/maven/**"]
reports = ["target/surefire-reports/*.xml"]

[[tasks]]
id = "test"
name = "Run tests"
description = "Run the unit tests after changing source or test code."
command = "mvn test"
reports = ["target/surefire-reports/*.xml"]

[[tasks]]
id = "push"
name = "Push to GitHub"
description = "Push the prepared commit to the configured remote when publication is authorized."
command = "git push"
```

These commands use the toolchain in the selected image. The Maven cache example
assumes the image or project configures its local repository under `.cache/maven`.
The push example requires a prepared Git checkout and granted credentials. A task
name or description does not provide either. The basic task needs no glob fields.

A command can be `mvn test`, `git push`, `python build.py`,
`sh scripts/build-book.sh`, or a multiline shell program. A separate script file is
optional. Run the exact frozen string with `/bin/sh -c` in the project root by
default. Select an image providing that shell; a command can explicitly invoke
another interpreter. Do not interpolate model selectors, paths, or secrets into the
command string. Pass additional data through literal environment values or files.

There are no task references, ordered step objects, dependency graphs, named runtime
profiles, cache profiles, or delivery/test wrappers in this proposal. A command or
ordinary project script expresses any sequence it needs. The agent may run one task,
inspect the result, and choose another after the first container has shut down.

## Optional file globs

| Field | Meaning |
| --- | --- |
| `products` | Retain matching generated outputs and make successful outputs available in Files. |
| `cache` | Restore/save matching disposable build or dependency files. |
| `reports` | Collect matching test reports and import identifiable case results into verification. |

All three default to empty. Logs and changes to authorized source/test files are
captured by the harness independently. Existing project files remain canonical.
Unmatched new files are discarded with the container.

Globs classify files, not task types or automatic triggers. A task can have all
three fields, any subset, or none. `mvn install` may emit useful test reports too.
Use project-relative patterns with one documented glob syntax. Reject conflicting
product/cache/report matches and protected paths. See [storage](files-storage.md).

Reports initially accept JUnit XML. The report importer uses the existing ledger's
test identities to find criteria; task settings do not repeat those mappings. The
case names, selection behavior, and treatment of missing results are specified in
[verification](generation-testing.md#selection-and-results).

## Environment and limits when needed

The default is a published Jazyk image with a documented tool inventory. A task can
override the image directly. There is no separate environment object to define.
E.g.:

```toml
[[tasks]]
id = "book"
name = "Create book"
description = "Render the approved chapters into PDF and EPUB for download."
command = "sh scripts/build-book.sh"
image = "ghcr.io/example/book-tools:1"
products = ["dist/book.pdf", "dist/book.epub"]
timeout_seconds = 1800
```

Optional advanced fields are `image`, project-relative `cwd`, `size`,
`timeout_seconds`, non-secret `env`, and credential references `secrets`. Keep them
in the task row. Expose effective defaults and host caps before starting. Resolve
image tags to digests and record image/platform, command, task definition, referenced
input files, and input revision with the run. Unsupported images fail explicitly.

Host policy owns provider choice, credentials, network grants, account limits, and
billing. The agent can choose within those limits, not change them. A command that
needs Git history requests the host's supported checkout capability; snapshots do
not copy credential-bearing Git config or the user's complete `.git` directory.

Descriptions help the model choose work. They are not security classifications.
Calling a command "Run tests" cannot grant it publication credentials. Changing a
task does not change the command, limits, or grants of a run already submitted.

## Simple cache behavior

Restore a compatible cache before the command. Save a new cache only after exit
zero, with no cancellation, timeout, or setup failure. Failed or cancelled attempts
leave the prior cache intact. Collection and teardown always have fixed bounds.

There is no "build succeeded but tests failed" checkpoint inside a command. If
`mvn install` fails during tests, it does not save a new cache. This is an accepted
simplicity tradeoff. A project may use separate build and test tasks when useful;
they run in fresh containers and can each use a disposable cache. The first release
does not add step configuration to optimize this case.

## GitHub sync shortcut

Keep the previously requested no-container shortcut as a host-provided task in the
same task picker and run history. Connecting a repository and branch offers an
optional `Sync to GitHub` row with a stable ID, name, and description. Its trusted
handler publishes a canonical snapshot without executing project shell commands.
Store the binding with the existing GitHub connection, not a new delivery table.

This is the one built-in exception to user-authored command execution. It is marked
as managed and is not an editable command row. A settings patch cannot replace its
handler or fabricate a trusted task ID. Do not detect the text `git push` and divert
it to a privileged service operation. A user task containing `git push` runs as an
ordinary command with its own granted checkout and credentials.

Repository, branch, and optional include/exclude globs are visible in connection
settings and in the run preview. GitHub sign-in is not repository write permission.
Several saved targets can supply several managed rows. Each has its own binding,
expected remote head, and publication authority.

Sync is one-way in v1. Include canonical authored/generated files and promoted
products by default. Exclude secrets, Git internals, cache/scratch, conversations,
and mutable compiler operational state. `.jazyk/jazyk.toml` and `.jazyk/.gitignore`
remain eligible. Graph exports must be explicit artifacts.

Preserve remote-only files, compare the expected head, and never force-push. A
conflict needs inspection/import. Uncertain external writes need reconciliation,
not automatic repetition. Binary products require the shared binary manifest and
size-aware Git blob support; larger outputs can use downloads or scripted uploads.

## How generation maintains tasks

The model receives relevant prose, the flat task list, bound tests, capabilities,
and effective limits. It reuses tasks by meaning, writes an optional script when a
command becomes unwieldy, and patches only the needed task rows. IDs stay stable
when names change. Record requirement-to-task and test-to-task links in generation
metadata, not duplicate editable definitions.

Validate unique IDs, nonempty name/description/command, supported optional fields,
globs, paths, and capabilities. If the command explicitly invokes a project script,
check it where feasible, but a task must not require a script path. A missing
executable at runtime is a setup/runner failure with logs.

Settings edits do not execute tasks or grant credentials. Document-set test bindings
refer to a project-level task ID and their own exact identities. The same task can
serve several sets without copying its command. Preserve unrelated TOML and comments.
Existing inline delivery commands can transfer directly into task commands; do not
create script files solely for conversion.
