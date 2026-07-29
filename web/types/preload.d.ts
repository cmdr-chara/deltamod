export {};

type ProgressEvent = {
    operationId: string;
    phase: string;
    completed: number;
    total: number;
    currentItem: string;
};

type OfficialProfileSummary = {
    exists: boolean;
    sourceRoot: string;
    destinationRoot: string;
    version: string | null;
    adapter?: string;
    installations?: number;
    mods?: number;
    themes?: number;
    fileCount?: number;
    totalBytes?: number;
    availableBytes?: number | null;
    canImport?: boolean;
};

type Unsubscribe = () => void;

declare global {
    interface Window {
        communityAPI: {
            app: {
                version(): Promise<string>;
                platform(): Promise<{ platform: NodeJS.Platform; release: string; version: string }>;
                minimize(): Promise<void>;
                toggleFullscreen(): Promise<void>;
                openMaintainerProfile(): Promise<void>;
            };
            profile: {
                summary(): Promise<OfficialProfileSummary>;
                import(operationId: string): Promise<{
                    operationId: string;
                    manifest: Record<string, unknown>;
                    restartRequired: boolean;
                }>;
                cancel(operationId: string): Promise<boolean>;
                onProgress(callback: (event: ProgressEvent) => void): Unsubscribe;
            };
            updates: {
                check(): Promise<unknown>;
                install(): Promise<void>;
                ignore(): Promise<void>;
            };
            tools: {
                undertaleModToolStatus(): Promise<{
                    supported: boolean;
                    configured: boolean;
                    executableName: string | null;
                    cliConfigured: boolean;
                    cliExecutableName: string | null;
                }>;
                chooseUndertaleModTool(): Promise<{
                    configured: boolean;
                    executableName: string | null;
                    canceled: boolean;
                }>;
                openInstallationInUndertaleModTool(installationIndex: string): Promise<{
                    launched: boolean;
                    canceled?: boolean;
                    executableName?: string;
                    dataFileName?: string;
                    workspacePath?: string;
                    sourceSha256?: string;
                    workCopy?: boolean;
                }>;
            };
        };
        electronAPI: {
            invoke<T = unknown>(channel: string, data?: unknown[]): Promise<T>;
        };
        preloadAPI: {
            onPage(callback: (page: string) => void): Unsubscribe;
            onAudio(callback: (enabled: boolean) => void): Unsubscribe;
            onGPL(callback: (message: string) => void): Unsubscribe;
            onUpdateAvailable(callback: (details: unknown) => void): Unsubscribe;
            onDDS(callback: (progress: unknown) => void): Unsubscribe;
            onThemeChange(callback: (theme: unknown) => void): Unsubscribe;
            onUpdateProgress(callback: (progress: unknown) => void): Unsubscribe;
            onRefresh(callback: () => void): Unsubscribe;
            onFinishedPatch(callback: (mods: unknown) => void): Unsubscribe;
            onDLMODProgress(callback: (progress: unknown) => void): Unsubscribe;
            onProtocolDownloadProgress(callback: (event: ProgressEvent) => void): Unsubscribe;
            onProfileImportProgress(callback: (event: ProgressEvent) => void): Unsubscribe;
            onGameImportProgress(callback: (event: ProgressEvent) => void): Unsubscribe;
            onHashProgress(callback: (event: ProgressEvent) => void): Unsubscribe;
            onWRA(callback: (message: string) => void): Unsubscribe;
            onLeaveControllerMode(callback: () => void): Unsubscribe;
        };
    }
}
