// VS Code extension: launches `jazyk lsp` and forwards LSP traffic. The extension does no
// analysis itself. Explicit commands manage document sets and request compilation.
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
  context.subscriptions.push(
    vscode.commands.registerCommand('jazyk.markDocuments', (uri: vscode.Uri) => markSelection(uri, 'docs')),
    vscode.commands.registerCommand('jazyk.markSource', (uri: vscode.Uri) => markSelection(uri, 'source')),
    vscode.commands.registerCommand('jazyk.compileSets', compileSets)
  );

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
    // Traceability also attaches to generated source and test files. Language
    // grammars and other language servers continue to own their language features.
    documentSelector: [{ scheme: 'file' }],
    synchronize: {
      // The editor owns file watching (native FSEvents/inotify): source documents for
      // anchoring, and the store's generation file so a committed build repaints
      // instantly without server-side polling.
      fileEvents: [
        vscode.workspace.createFileSystemWatcher('**/*.md'),
        vscode.workspace.createFileSystemWatcher('**/.jazyk/*/{status,build}.yaml'),
        vscode.workspace.createFileSystemWatcher('**/.jazyk/jazyk.toml'),
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


type DocumentSet = { id: string; name: string; docs: string[]; roots: string[]; source: string[] };
type DocumentSets = { hash: string; sets: DocumentSet[] };
async function execute<T>(command: string, arg: unknown = {}): Promise<T> {
  if (!client) throw new Error('Jazyk language server is not running.');
  const result = await client.sendRequest<T & { error?: string }>('workspace/executeCommand', { command, arguments: [arg] });
  if (result.error) throw new Error(result.error);
  return result;
}
async function compileSets(): Promise<void> {
  try {
    const snapshot = await execute<DocumentSets>('jazyk.documentSets');
    const choice = await vscode.window.showQuickPick([{ label: 'All dirty document sets', id: '' }, ...snapshot.sets.map(s => ({ label: s.name, id: s.id }))]);
    if (choice) await execute('jazyk.compile', choice.id ? { setId: choice.id } : {});
  } catch (e) { void vscode.window.showErrorMessage(String(e)); }
}
async function markSelection(uri: vscode.Uri, role: 'docs' | 'source'): Promise<void> {
  try {
    if (!uri) return;
    const folder = vscode.workspace.getWorkspaceFolder(uri);
    if (!folder) return;
    let relative = path.relative(folder.uri.fsPath, uri.fsPath).split(path.sep).join('/');
    if (!relative || relative.startsWith('../')) return;
    if ((await vscode.workspace.fs.stat(uri)).type & vscode.FileType.Directory) relative += '/**';
    const snapshot = await execute<DocumentSets>('jazyk.documentSets');
    const choice = await vscode.window.showQuickPick([{ label: 'New document set…', id: '' }, ...snapshot.sets.map(s => ({ label: s.name, id: s.id }))]);
    if (!choice) return;
    const existing = snapshot.sets.find(s => s.id === choice.id);
    const name = existing?.name ?? await vscode.window.showInputBox({ prompt: 'Document set name' });
    if (!name) return;
    let docs = existing?.docs ?? [];
    if (role === 'source' && !existing) {
      const selection = await vscode.window.showInputBox({ prompt: 'Documents for this set, one pattern per comma', placeHolder: 'docs/feature.md, docs/feature/**' });
      if (!selection) return;
      docs = selection.split(',').map(s => s.trim()).filter(Boolean);
    }
    const set = { ...existing, name, docs, roots: existing?.roots ?? [], source: existing?.source ?? [] };
    set[role] = [...new Set([...set[role], relative])];
    const edit = { baseHash: snapshot.hash, set };
    const preview = await execute<DocumentSets>('jazyk.previewDocumentSet', edit);
    const affected = preview.sets.find(s => s.name === name) as DocumentSet & { documents?: string[]; sourceFiles?: string[] };
    const apply = await vscode.window.showInformationMessage(`${name}: ${affected?.documents?.length ?? 0} documents, ${affected?.sourceFiles?.length ?? 0} source files.`, 'Save selections');
    if (apply) await execute('jazyk.saveDocumentSet', edit);
  } catch (e) { void vscode.window.showErrorMessage(String(e)); }
}
