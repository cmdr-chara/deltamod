export type DeltamodAssetKind = 'app' | 'theme' | 'packet';
export type DeltamodBackendUnsubscribe = () => void;

export interface DeltamodBackend {
    invoke<T = unknown>(channel: string, data?: unknown[]): Promise<T>;
    invokeOptional<T = unknown>(channel: string, data?: unknown[], fallback?: T): Promise<T>;
    isCommandAvailable(channel: string): boolean;
    on(channel: string, callback: (payload: unknown) => void): DeltamodBackendUnsubscribe;
    assetUrl(kind: DeltamodAssetKind, path: string): string;
}

declare global {
    interface Window {
        deltamodBackend: DeltamodBackend;
    }
}
