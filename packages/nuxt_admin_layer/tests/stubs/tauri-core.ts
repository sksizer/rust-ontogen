// Stand-in for `@tauri-apps/api/core` (aliased in vitest.config.ts): the
// event-subscription tests drive the generated IPC transport without Tauri.

export class Channel<T> {
  onmessage: (message: T) => void = () => {}
}

export const invokeCalls: { command: string; args: Record<string, unknown> | undefined }[] = []
let nextResult: unknown = null

/** The value the next `invoke` resolves to. */
export function setInvokeResult(value: unknown): void {
  nextResult = value
}

export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  invokeCalls.push({ command, args })
  return nextResult as T
}
