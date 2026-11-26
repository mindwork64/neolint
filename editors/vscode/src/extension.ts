import * as path from 'node:path';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    const settings = vscode.workspace.getConfiguration('neolint');
    const command = settings.get<string>('serverPath', 'neolint-lsp');
    const args = settings.get<string[]>('serverArgs', []);
    const configuredPath = settings.get<string>('configPath', '').trim();
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    const configPath = configuredPath
        ? path.resolve(workspaceRoot ?? process.cwd(), configuredPath)
        : undefined;

    const serverOptions: ServerOptions = {
        command,
        args,
        options: workspaceRoot ? { cwd: workspaceRoot } : undefined,
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [
            { scheme: 'file', pattern: '**/*.lsl' },
        ],
        initializationOptions: { configPath },
        outputChannelName: 'NeoLint',
    };

    client = new LanguageClient('neolint', 'NeoLint', serverOptions, clientOptions);
    context.subscriptions.push(client);

    try {
        await client.start();
    } catch (error) {
        client.outputChannel.show(true);
        const detail = error instanceof Error ? error.message : String(error);
        void vscode.window.showErrorMessage(
            `NeoLint language server could not start: ${detail}`,
        );
        throw error;
    }
}

export async function deactivate(): Promise<void> {
    if (client) {
        const running = client;
        client = undefined;
        await running.stop();
    }
}
