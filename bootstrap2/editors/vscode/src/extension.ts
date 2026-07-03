// VS Code extension: launches `jazyk lsp` and forwards LSP traffic. The extension does no
// analysis itself. The server is read-only: it maps the graph store to editor positions
// and never compiles; run `jazyk compile` or `jazyk watch` beside the editor to rebuild.
// Mirrors docs2/frontends/lsp.md.
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
    documentSelector: [
      { scheme: 'file', language: 'jazyk' },
      { scheme: 'file', language: 'markdown' },
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.md'),
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

// Resolution order: an explicit setting wins; otherwise prefer the workspace's own
// bootstrap2 build (release, then debug); otherwise rely on PATH.
function resolveBinary(configured: string | undefined): string {
  if (configured && configured.trim().length > 0) {
    return configured;
  }
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    for (const rel of [
      path.join('bootstrap2', 'target', 'release', 'jazyk'),
      path.join('bootstrap2', 'target', 'debug', 'jazyk'),
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
