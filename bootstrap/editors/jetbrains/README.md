# Jazyk for JetBrains IDEs

One plugin serves IntelliJ IDEA, PhpStorm, WebStorm, PyCharm, GoLand, RustRover,
CLion, Rider, and RubyMine. It uses the free LSP4IJ client, not the IDE's native
LSP API. Markdown keeps the IDE's existing syntax support.

Install LSP4IJ (`com.redhat.devtools.lsp4ij`) from the IDE's plugin marketplace.
Build with JDK 21 or newer. The wrapper pins Gradle 9.3.1 and its distribution checksum:

```sh
./gradlew check buildPlugin -PideHome=/path/to/idea -Plsp4ijHome=/path/to/plugins/lsp4ij
```

On macOS, `ideHome` is the application's `Contents` directory. Use a JDK at least
as new as the IDE's required Java version. LSP4IJ 0.21.0 and IntelliJ 2026.2 are
the initial validation baseline. The plugin declares an IntelliJ platform 2024.2
minimum; other versions require validation against their installed LSP4IJ.

The output is `build/libs/jazyk-jetbrains.jar` and an installable ZIP under
`build/distributions/`. Run `jazyk idea` (or the other IDE's launcher choice).
It records the executable in `.jazyk/ide.json` and installs the built JAR into
that product's plugin directory when the product can be identified. Otherwise,
use Settings → Plugins → Install Plugin from Disk and select the ZIP.
Restart an already running IDE after installation.

The adapter activates only when the IDE project base directory is inside a
Jazyk project. It starts the server in the discovered Jazyk root. An explicit
launcher-written command wins; otherwise `jazyk` comes from the IDE environment.
No shell command is constructed. The ACP agent registration is independent.

Check the LSP Consoles tool window for Jazyk's initialization. Open a Markdown
file with a missing local link and verify the `broken-link` diagnostic. Test
hover, definitions, references, completion, quick fixes and refresh against a
compiled graph. No model call is needed to test missing links.
