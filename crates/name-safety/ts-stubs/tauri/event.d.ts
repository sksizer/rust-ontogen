// The slice of `@tauri-apps/api/event` the generated IPC transport calls.
export declare function listen<T>(event: string, handler: (event: { payload: T }) => void): Promise<() => void>;
