# Native distribution

Jazyk ships as native archives on GitHub Releases and as the `jazyk` npm package.
The native channels contain the same native build. The npm package is a Node launcher.
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

## Additional package managers

- Homebrew: `brew install jazykorg/tap/jazyk`.
- Scoop: `scoop bucket add jazyk https://github.com/JazykOrg/scoop-bucket`,
  then `scoop install jazyk/jazyk`.
- Cargo: `cargo install --locked jazyk` with Rust 1.99.1 or newer and the
  native compiler tools for the platform.

Homebrew and Scoop install the complete GitHub Release distribution, including
PDFium and notices. Homebrew keeps it in `libexec`. A wrapper in `bin` executes
the real binary so PDFium resolves beside it on macOS and Linux. Scoop creates a shim for the executable. Both retain `lib/` beside it.
Homebrew supports the four macOS and glibc Linux targets. Scoop supports Windows x64.

Cargo builds from source. Its package contains the canonical embedded documentation,
benchmark fixtures and task helper sources. Release preparation copies those files
into a standalone staging tree without changing the canonical source layout.
The patched compaction dependency is published as `jazyk-goose-context-management`
under its Apache-2.0 license. The installed command remains `jazyk`.
Graphviz uses the dependency's default prebuilt download when no local library is
available. Cargo installs the executable only. For PDF input, install a compatible
PDFium library and set `JAZYK_PDFIUM` to its path. Other input formats and diagram
rendering do not require PDFium. Native archives include PDFium automatically.

`scripts/release/source-package.py` prepares the standalone Cargo source tree.
Run `cargo package` and `cargo publish --dry-run` in that tree before publication.
Publish the patched compaction crate before Jazyk. Registry versions are immutable.

`scripts/release/package-managers.py` generates a Homebrew formula and Scoop
manifest from a published release manifest. It verifies archive hashes against
`SHA256SUMS` and pins versioned URLs. Homebrew package revisions come from
`scripts/release/homebrew-revisions.json` and reset for each upstream version. The `release-packages.yml` workflow validates
source packages and generates package-manager metadata. Publication uses the
existing published GitHub Release and its exact source commit. Metadata generation
uses the workflow revision. Source packaging uses the published source commit. It can be retried
independently of npm. The workflow publishes Cargo packages with
`CARGO_REGISTRY_TOKEN` and retains Homebrew and Scoop manifests as an artifact. Commit those manifests to
`JazykOrg/homebrew-tap` and `JazykOrg/scoop-bucket` with the owner signing identity.
A first release publishes the compaction crate before Jazyk.
Prereleases do not update the stable Homebrew or Scoop manifests.

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

## Versions

Every build names its version in one string. `jazyk --version`, `jazyk version`
([cli](./cli.md#jazyk-version)), `GET /api/ping`, the MCP and LSP `serverInfo`, and
Jazyk Cloud report the same string.

- A release build is the release number, e.g. `0.2.2`.
- Any other build is unreleased: the last release before it, `+`, the commits since
  that release, and the commit. E.g. `0.2.2+31.ge0fff82` is 31 commits after `0.2.2`.
- A build that cannot count those commits (a checkout without the release tags)
  omits the count: `0.2.2+ge0fff82`.
- A build with no evidence of where it came from reads `0.2.2+unknown`.

The build finds it in this order:

1. `JAZYK_BUILD_VERSION` (and `JAZYK_BUILD_COMMIT`) in the build's environment,
   used as given. The release workflow sets it to the version it releases. A builder
   that compiles a checkout without history, e.g. Jazyk Cloud, sets it.
2. A packaged crate (`cargo install jazyk`): the `Cargo.toml` version, a release.
3. The Jazyk Git checkout the source sits in: `git describe` against the
   `v<version>` tags.
4. Otherwise `<Cargo.toml version>+unknown`.

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
