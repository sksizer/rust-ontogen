// Stand-in for `@tauri-apps/api/event` (aliased in vitest.config.ts).

export async function listen(_event: string, _handler: (event: { payload: unknown }) => void): Promise<() => void> {
  return () => {}
}
