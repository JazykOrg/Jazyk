// VS Code extension: launches `jazyk lsp` and forwards LSP traffic. The extension does no
// analysis itself. The server is read-only: it maps the graph store to editor positions
// and never compiles; run `jazyk compile` or `jazyk watch` beside the editor to rebuild.
// Mirrors docs/frontends/lsp.md.
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  await startClient();

  // The server launch path comes from settings at start time, so restart the server
  // whenever a jazyk.* setting changes.
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration('jazyk')) {
        await restartClient();
      }
    })
  );

  // A command to restart on demand.
  context.subscriptions.push(
    vscode.commands.registerCommand('jazyk.restartServer', restartClient)
  );

  // Freeform answers to prompted diagnostics: base LSP has no input surface, so
  // the extension supplies one and forwards to the server's command. The options
  // themselves arrive as ordinary quick fixes without this command.
  // Mirrors docs/frontends/lsp.md#capabilities.
  context.subscriptions.push(
    vscode.commands.registerCommand('jazyk.answer', async () => {
      if (!client) {
        return;
      }
      const id = await vscode.window.showInputBox({
        prompt: 'Diagnostic id (e.g. diag:contradiction-1)',
        placeHolder: 'diag:…',
      });
      if (!id) {
        return;
      }
      const text = await vscode.window.showInputBox({
        prompt: 'Your answer, in your own words',
      });
      if (!text) {
        return;
      }
      const result = await client.sendRequest('workspace/executeCommand', {
        command: 'jazyk.answerDiagnostic',
        arguments: [{ id, text }],
      });
      const r = result as { status?: string; error?: string } | null;
      if (r?.error) {
        void vscode.window.showErrorMessage(`jazyk: ${r.error}`);
      } else {
        void vscode.window.showInformationMessage(
          `jazyk: answer recorded (${r?.status ?? 'ok'}); the agent is handling it`
        );
      }
    })
  );
}

async function startClient(): Promise<void> {
  const config = vscode.workspace.getConfiguration('jazyk');
  const jazykPath = resolveBinary(config.get<string>('server.path'));

  const args = ['lsp'];
  const serverOptions: ServerOptions = {
    run: { command: jazykPath, args, transport: TransportKind.stdio },
    debug: { command: jazykPath, args, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    // Markdown files keep VS Code's built-in markdown language (grammar, link
    // following, preview); the extension only attaches the LSP client to them.
    documentSelector: [{ scheme: 'file', language: 'markdown' }],
    synchronize: {
      // The editor owns file watching (native FSEvents/inotify): source documents for
      // anchoring, and the store's generation file so a committed build repaints
      // instantly without server-side polling.
      fileEvents: [
        vscode.workspace.createFileSystemWatcher('**/*.md'),
        vscode.workspace.createFileSystemWatcher('**/jazyk-out/status.yaml'),
      ],
    },
    middleware: {
      // The server links the walk's pages as plain file:// URIs so any client
      // navigates; here they open in the markdown preview beside the document, where
      // the card's relative links click through to levels and diagrams.
      provideHover: async (document, position, token, next) => {
        const hover = await next(document, position, token);
        if (!hover) {
          return hover;
        }
        hover.contents = hover.contents.map(previewWalkLinks);
        return hover;
      },
    },
  };

  client = new LanguageClient('jazyk', 'Jazyk', serverOptions, clientOptions);
  await client.start();
}

async function restartClient(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
  await startClient();
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

// A markdown link to a walk page under <out>/docsgen/: an entity card
// (entities/), a diagram page (diagrams/<kind>/), or a level page (levels/). The
// requirements document sits directly under docsgen/ and keeps its file link, which
// lands on the heading's line. Mirrors docs/frontends/lsp.md#capabilities.
const WALK_PAGE_LINK =
  /\]\((file:\/\/[^)\s]*\/docsgen\/(?:entities|diagrams|levels)\/[^)\s]+\.md)\)/g;

// Rewrite the walk page links of one hover block into command links that open the
// page in the markdown preview to the side. Images and every other link stay as the
// server sent them; the block is trusted for that one command only.
function previewWalkLinks(
  block: vscode.MarkdownString | vscode.MarkedString
): vscode.MarkdownString | vscode.MarkedString {
  if (!(block instanceof vscode.MarkdownString)) {
    return block;
  }
  const value = block.value.replace(WALK_PAGE_LINK, (_match, href: string) => {
    const args = encodeURIComponent(JSON.stringify([vscode.Uri.parse(href)]));
    return `](command:markdown.showPreviewToSide?${args})`;
  });
  if (value === block.value) {
    return block;
  }
  const out = new vscode.MarkdownString(value, block.supportThemeIcons);
  out.isTrusted = { enabledCommands: ['markdown.showPreviewToSide'] };
  out.supportHtml = block.supportHtml;
  out.baseUri = block.baseUri;
  return out;
}

// Resolution order: an explicit setting wins; otherwise prefer the workspace's own
// bootstrap build (release, then debug); otherwise rely on PATH.
function resolveBinary(configured: string | undefined): string {
  if (configured && configured.trim().length > 0) {
    return configured;
  }
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    for (const rel of [
      path.join('bootstrap', 'target', 'release', 'jazyk'),
      path.join('bootstrap', 'target', 'debug', 'jazyk'),
    ]) {
      const candidate = path.join(folder.uri.fsPath, rel);
      try {
        fs.accessSync(candidate, fs.constants.X_OK);
        return candidate;
      } catch {
        // keep looking
      }
    }
  }
  return 'jazyk';
}
