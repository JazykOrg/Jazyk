# GitHub accounts, publishing, and Actions

Status: proposed, with primary-source research checked 2026-10-03. Use GitHub early
in the cloud implementation, not as a post-beta storage add-on.

## Login and repository access

Recommend one GitHub App. Use its OAuth user flow for `Continue with GitHub` and its
installation grants for selected repositories. This satisfies GitHub-only sign-up
without requiring a broad OAuth App repository token. GitHub Apps support OAuth user
identity as well as repository-scoped installation access.
[GitHub App and OAuth App comparison](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/differences-between-github-apps-and-oauth-apps).

Store GitHub's stable numeric user id as the identity, not username or email. Use
the documented authorization-code flow with state, PKCE, and exact callback binding,
through maintained libraries. Cloud sessions are Jazyk credentials; a browser or
connector does not receive the App private key or installation token.
[User access token flow](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app).

Login creates the account and project list. `Connect repository` obtains or selects
an installation and repository binding. Organization policy may require another
person to approve installation. Login alone does not authorize publishing, and it
does not mean Jazyk can create or access every repository belonging to that user.

Offer `Use existing repository` first. A later create-repository shortcut needs its
own permission flow and visibility selection. Do not create a repository implicitly
when a project or account is created. A cloud project may stay in cloud storage and
only use GitHub as its delivery destination.

## Permissions and credentials

Resolve tokens in the cloud host, narrowly scoped to the selected installation and
repository. Refresh installation tokens as needed. Keep the App key and refresh
material encrypted outside project files. The delivery worker receives only the
specific run's required credentials, never a broad account token.

| Capability | Grant to plan and verify |
|---|---|
| Read source | Repository contents read |
| Publish commits | Contents write |
| Write workflow files | Contents and Workflows write |
| Dispatch/cancel a workflow | Actions write |
| Inspect runs/logs/artifacts | Actions read |
| Open a proposed workflow change as a PR | Pull requests write |

Workflow-file changes need their own permission. Validate this using the actual
commit/contents adapter, rather than discovering a missing grant after staging a
delivery. [Contents API permissions](https://docs.github.com/en/rest/repos/contents).
Dispatch accepts GitHub App tokens with Actions write.
[Workflow dispatch API](https://docs.github.com/en/rest/actions/workflows#create-a-workflow-dispatch-event).

Verify webhook signatures over the original request bytes, deduplicate delivery ids,
and scope events by installation/repository id. Reconcile periodically through the
API because webhooks can be missed or arrive out of order. Uninstall/revocation
blocks new work and surfaces unavailable access on existing attempts.

Accountless local Git delivery can use the user's existing Git credentials. Using
Jazyk Cloud's repository grant requires login. GitHub Actions minutes/storage belong
to the repository's GitHub account; they are not Jazyk container minutes. Jazyk model
use and any declared orchestration charge remain separate and visible.

## Container-free Git delivery

For GitHub, publish through blobs, tree, commit, and ref APIs. Prepare one commit
with all selected files and removals against an observed parent. Preserve paths
outside the delivery's managed scope. Update the destination ref without force.
This requires neither a working-tree checkout nor a project container.
[Git commit API](https://docs.github.com/en/rest/git/commits),
[reference API](https://docs.github.com/en/rest/git/refs).

Record the new commit id before updating the ref. Confirm the intended commit/tree
at the destination after publication. A non-fast-forward update becomes a conflict
or replan. GitHub's non-force update is a fast-forward guard, not a general
compare-and-swap on an arbitrary old SHA; do not promise stronger semantics.

Protected branches can require a PR. Show `Published branch; awaiting merge` instead
of claiming delivery to the protected branch. A merged PR must be checked for the
actual resulting tree/commit before any dependent Actions run. Existing push-triggered
workflows are disclosed effects of publication even when Jazyk itself does not dispatch.

Generic Git remotes use the same candidate and receipt contract. The local connector
can push through existing local credentials. For cloud-hosted publication, use a
maintained Git transport library/helper with a private temporary bare object store,
no checkout, hooks, or project commands. Git transport scratch is not a code runtime.
Start with HTTPS; SSH can follow when credential handling is specified. Reject local
paths, custom transport commands, redirects to private networks, and credential-bearing
URLs. Scope credentials to the validated remote. Do not hand-write the Git protocol.

A failed push with an uncertain response requires reading the remote ref before
retry. Commit history preserves graph state only if graph state was included in the
published tree. Default source delivery excludes private graph/operational exports.

## Actions delivery lifecycle

An Actions-backed script is one delivery attempt containing publication, dispatch,
and verification steps. Reuse the Git publisher internally; do not make users start
two independent deliveries to run a test script.

1. Stage a workflow and entrypoint with the model's normal workspace tools. Use
   `workflow_dispatch` initially and stable delivery/attempt inputs.
2. Publish/install the workflow through an explicit Git change. `workflow_dispatch`
   requires the workflow to exist on the default branch. If branch protection
   prevents this, wait for its merge. Publishing only to a new feature branch does
   not finish setup. [Manual workflow requirements](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).
3. Publish the pinned candidate to an agreed branch or unique attempt ref. Supply its
   exact commit SHA to the workflow. Checkout that SHA, not a moving branch tip.
4. Dispatch using the known workflow revision/ref, attempt id, and candidate SHA.
   Retain workflow identity separately from the source it checks out. Freeze and
   revalidate generated workflow/procedure content as part of preparation.
5. Persist the returned provider run id and URL. The current REST documentation
   describes a `200` response containing these values. Pin/test the API version;
   older installations may need event correlation instead.
   [Dispatch response](https://docs.github.com/en/rest/actions/workflows#create-a-workflow-dispatch-event).
6. Watch job/step status through webhooks and bounded polling. On completion, fetch
   logs, manifest, test reports, and selected artifacts. Verify their candidate identity.
7. Advance only the appropriate checkpoint after its required checks. A green
   workflow alone does not confirm an unrelated production destination.

Include `attempt_id` in workflow inputs/run naming and output manifests. If dispatch
times out before a receipt arrives, correlate runs before considering a retry.
Persist GitHub `run_attempt` when a run is retried; keep evidence from different
attempts distinct. For non-idempotent external actions, retain an unknown result
instead of automatically redispatching because no recent run was found.

Keep generated workflows small. They checkout exact source, select the declared
toolchain, invoke the entrypoint, and upload outputs even on test failure where
possible. Pin action revisions when generating executable workflow content. Use
repository/environment secrets or provider OIDC for deployment credentials.
Do not upload an App private key or Jazyk account token. Ordinary `GITHUB_TOKEN`
events have special trigger rules; verify trigger behavior instead of assuming a
push from any credential starts the same follow-up jobs.
[GitHub token behavior](https://docs.github.com/en/actions/concepts/security/github_token).

## Actions evidence and file access

GitHub supplies run/job status, downloadable logs, and artifact APIs. Use them for
watching and retained evidence. Do not promise a live byte-by-byte log stream when
only completed-job logs are available; link to GitHub for its native live view.
[Workflow runs API](https://docs.github.com/en/rest/actions/workflow-runs),
[workflow jobs API](https://docs.github.com/en/rest/actions/workflow-jobs).

Jazyk can read the exact repository candidate and files explicitly uploaded as
artifacts. It cannot list arbitrary current files or run a command inside a
GitHub-hosted runner. File inspection during execution is a container-runner
capability. A future custom Actions agent bridge is separate work, not required now.

The workflow uploads a manifest containing attempt id, candidate SHA/digest,
entrypoint/config identity, report paths, output digests, and test command status.
Match this with the authenticated repository/run receipt. The checked-out candidate
can differ from the workflow's own head SHA, so verify both roles explicitly.

Download and retain required evidence before GitHub artifacts expire. Expiry or a
deleted artifact means evidence is unavailable, not that a test passed or failed.
Reject archive traversal, oversized extraction, links escaping the artifact root,
and external XML entities. [Actions artifact API](https://docs.github.com/en/rest/actions/artifacts).

An artifact manifest is evidence from a particular run, not a trust upgrade for
arbitrary output. Compare bytes, expected report names, and configured checks.
Tests with known criterion bindings update verification only for the matching
candidate. Unmapped test results remain visible as run evidence.
