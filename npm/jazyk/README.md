# Jazyk

Jazyk compiles documentation into requirements, acceptance criteria, diagrams,
code and tests. See [jazyk.org](https://jazyk.org).

```sh
npx -y jazyk
npm install -g jazyk
jazyk --help
```

Node.js 22 or newer. macOS ARM64/x64, glibc Linux ARM64/x64 (glibc 2.35+),
and Windows x64. The platform package contains the native binary and PDFium.
No Rust toolchain is required. Local shell build/test commands on Windows need
`sh` on `PATH` (e.g. Git for Windows). The editor is at https://jazyk.dev.

The launcher uses `JAZYK_BINARY` when set, otherwise an exact-version optional
platform package, then a checksummed GitHub Release download cached locally.
`JAZYK_DOWNLOAD_BASE` overrides the release download base. Diagnostics use stderr.
No install scripts run. Do not omit optional dependencies for offline installs.

[Direct downloads](https://github.com/JazykOrg/Jazyk/releases/latest) need no Node.js.
Extract the whole archive and keep `lib/` beside the executable.
[Source and documentation](https://github.com/JazykOrg/Jazyk).
