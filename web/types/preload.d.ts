// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

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

type NexusQuotaWindow = {
    limit?: number | null;
    remaining?: number | null;
    resetAt?: string | null;
};

type NexusQuota = {
    daily?: NexusQuotaWindow;
    hourly?: NexusQuotaWindow;
};

type NexusStatus = {
    configured: boolean;
    connected: boolean;
    ssoAvailable: boolean;
    ssoPending: boolean;
    authMethod: 'sso' | null;
    name?: string;
    premium?: boolean;
    error?: string;
    code?: string;
    retryAfterMs?: number | null;
    retryAt?: string | null;
    quota?: NexusQuota;
};

type ModSourceBrowseError = {
    code: string;
    message: string;
    status?: number;
    retryAfterMs?: number | null;
    retryAt?: string | null;
    quota?: NexusQuota;
};

type ModSourceBrowseResponse = {
    ok: boolean;
    result?: unknown;
    error?: ModSourceBrowseError;
};

declare global {
    interface Window {
        communityAPI: {
            app: {
                version(): Promise<string>;
                platform(): Promise<{ platform: NodeJS.Platform; release: string; version: string }>;
                minimize(): Promise<void>;
                toggleFullscreen(): Promise<void>;
                openMaintainerProfile(): Promise<void>;
                shakeForEasterEgg(phase: 'slash' | 'numbers' | 'stop'): Promise<{
                    phase: 'slash' | 'numbers' | 'stop';
                    native: boolean;
                }>;
                quitForEasterEgg(): Promise<{ closing: boolean }>;
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
            modSources: {
                providers(): Promise<Array<{
                    id: 'gamebanana' | 'nexus' | 'moddb';
                    name: string;
                    available: boolean;
                    requiresAuthentication?: boolean;
                    catalogScope?: 'recent';
                    installMode?: 'manual';
                }>>;
                browse(request: {
                    provider: 'nexus' | 'moddb';
                    query?: string;
                    sort?: string;
                }): Promise<ModSourceBrowseResponse>;
                nexusStatus(): Promise<NexusStatus>;
                startNexusSso(): Promise<NexusStatus>;
                cancelNexusSso(): Promise<boolean>;
                clearNexusKey(): Promise<boolean>;
                open(request: {
                    provider: 'gamebanana' | 'nexus' | 'moddb';
                    url: string;
                }): Promise<boolean>;
                downloadNexus(request: {
                    modId: string | number;
                    operationId: string;
                    sourceUrl: string;
                }): Promise<unknown>;
                onProgress(callback: (event: ProgressEvent) => void): Unsubscribe;
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
