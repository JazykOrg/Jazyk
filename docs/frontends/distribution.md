# Native distribution

Jazyk ships as native archives on GitHub Releases and as the `jazyk` npm package.
Both channels contain the same native build. The npm package is a Node launcher.
Every npm package declares `https://jazyk.org` as its homepage.
The editor remains at `jazyk.dev`.

## Installation

- Run `npx -y jazyk` with Node.js 22 or newer.
- Install with `npm install -g jazyk`, then run `jazyk`.
- Download an archive from GitHub Releases. Verify its SHA-256 against
  `SHA256SUMS`, extract the complete directory, and put it on `PATH`.
  Keep `lib/` beside the executable. Direct downloads need neither Node nor Rust.

Targets are macOS ARM64 and x64, glibc Linux ARM64 and x64, and Windows x64.
Linux requires glibc 2.35 or newer. Musl is not supported. The archive includes
PDFium and its notices. Rust dependency license files and the pinned native
renderer notices are included in `THIRD_PARTY_NOTICES`. Graphviz is linked into the executable. Optional execution
backends such as Docker retain their installation requirements. Executing local
build and test commands on Windows requires `sh` on `PATH`, such as Git for Windows.

## npm launcher

Resolution is `JAZYK_BINARY` → exact-version optional platform package → cached
GitHub Release download for the same version and platform. The launcher rejects
unsupported targets and verifies `SHA256SUMS` before extracting a download. Cache
installation is atomic and includes the complete distribution.
`JAZYK_DOWNLOAD_BASE` overrides the release download base. `XDG_CACHE_HOME` controls
the Unix cache root. Windows uses `LOCALAPPDATA` when available.

The child inherits arguments, working directory, environment and standard streams.
Termination signals and exit status are forwarded. Launcher diagnostics use stderr.
MCP and ACP stdout remains protocol data. Windows extraction uses the built-in
`System32/tar.exe`, independent of a Git Bash installation. Platform packages declare `os`, `cpu`,
and Linux `libc`. All optional dependencies use the exact launcher version.

## Source checkout

`.gitattributes` keeps source and embedded text payloads at LF on every platform.
Other files retain their committed bytes, including opaque document fixtures.
Release metadata from Cargo is UTF-8.

## Manual releases

`.github/workflows/release.yml` accepts `workflow_dispatch` only, on `master`.
Branch pushes and tag pushes do not publish. The version input must match
`bootstrap/Cargo.toml`. The default run validates without publishing. A first publication requires the
bootstrap npm credential before any release builds start. The workflow checks
its npm account identity up front. Package publish permission is checked during
publication.

The workflow tests and builds all targets, bundles pinned and checksummed PDFium,
tests the extracted distributions and local npm installations, and prepares all
archives and npm tarballs from those builds. Failed native jobs retain their
dependency cache for a retry. A manifest records the source commit,
version and hashes. Publishing consumes those artifacts without rebuilding.

With `publish` enabled it calls two reusable workflows:

- `release-github.yml` creates `v<version>` at the validated commit and publishes
  archives, checksums, npm tarballs and the manifest as release assets.
- `release-npm.yml` follows the GitHub Release. It publishes platform packages first
  and the launcher last. All platform packages are submitted before waiting for
  registry visibility. The launcher is submitted only after every platform is
  publicly available and its integrity matches. Stable versions use `latest`.
  Prereleases use `next` and a GitHub prerelease.

Drafts are located through the release list before their tag is published.
Existing versions must match the prepared bytes. Published content is never
replaced. Rerun failed jobs to reuse retained artifacts after partial publication.
Registry checks allow up to 30 minutes per publication phase for npm scanning.
A timeout leaves the accepted package pending at npm. Wait for it to become
public before retrying. The npm workflow also accepts a manual `source_run_id`
for a completed `release.yml` run on `master`. It downloads that run's artifacts,
checks their source commit and exact manifest against the published GitHub
Release, and resumes npm publication without rebuilding.
A new build of an existing release must match its hashes. An npm failure leaves
usable direct downloads and a failed workflow. Corrections require a new version.

Initial publication requires npm permission for every package name. `NPM_TOKEN`
can hold a short-lived publishing credential. Enable `Bypass two-factor
authentication` when creating it. Grant `Read and write (publish and stage)`
access to all six package names, including permission to create them on initial
publication. Successful account authentication does not confirm publishing
permission. `EOTP` requires correcting the credential or interactive authentication.

The configured token remains in use until it expires or is replaced. For token-free
publication after the first full release, configure npm trusted publishing for
every package with repository `JazykOrg/Jazyk` and calling workflow `release.yml`,
then remove the token.
The publishing job requests OIDC. Provenance is enabled only when the source
repository is public. npm does not support provenance for private repositories.
Login and 2FA are interactive.

Local validation covers launcher behavior, packages, workflow syntax and docs.
Hosted validation runs native Rust tests in the release profile, the WASM check
and Go helper tests.
Smoke checks cover PDF loading and diagram rendering without model calls, both
in an extracted archive and in the launcher download cache.
After publication, verify clean npm and archive installations from public URLs.

The [site](../site.md) offers npm commands, named platform download links through
GitHub's latest-release URLs, checksums and all releases. These URLs advance on
stable releases. Site deployment remains independent. See [CLI](cli.md),
[GUI](gui.md) and [WASM](wasm.md) for execution surfaces.
