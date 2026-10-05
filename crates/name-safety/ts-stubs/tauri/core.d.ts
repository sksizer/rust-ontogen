// The slice of `@tauri-apps/api/core` the generated IPC transport calls, so
// tsc can check it without installing Tauri.
export declare function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;

export declare class Channel<T = unknown> {
  constructor();
  id: number;
  onmessage: (message: T) => void;
}
