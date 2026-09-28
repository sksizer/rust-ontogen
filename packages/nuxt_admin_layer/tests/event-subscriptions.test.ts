/**
 * The generated `subscribeX` methods, driven against a fake EventSource (HTTP)
 * and a stubbed Tauri `invoke`/`Channel` (IPC). The transport under test is
 * `fixtures/event-transport.generated.ts`, which the Rust test
 * `ts_event_transport_fixture_is_current` keeps equal to the generator's output.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { Channel, invokeCalls, setInvokeResult } from './stubs/tauri-core'
import { createHttpTransport, createIpcTransport } from './fixtures/event-transport.generated'

class FakeEventSource {
  static instances: FakeEventSource[] = []
  readonly url: string
  onopen: (() => void) | null = null
  onerror: ((err: Event) => void) | null = null
  closed = false
  private listeners = new Map<string, ((event: MessageEvent) => void)[]>()

  constructor(url: string) {
    this.url = url
    FakeEventSource.instances.push(this)
  }

  addEventListener(name: string, fn: (event: MessageEvent) => void): void {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), fn])
  }

  close(): void {
    this.closed = true
  }

  open(): void {
    this.onopen?.()
  }

  emit(name: string, data: unknown, lastEventId = ''): void {
    const event = { data: JSON.stringify(data), lastEventId } as MessageEvent
    for (const fn of this.listeners.get(name) ?? []) fn(event)
  }

  fail(): void {
    this.onerror?.(new Event('error'))
  }
}

function latest(): FakeEventSource {
  const es = FakeEventSource.instances.at(-1)
  if (!es) throw new Error('no EventSource opened')
  return es
}

describe('HTTP subscribeX', () => {
  beforeEach(() => {
    FakeEventSource.instances = []
    vi.stubGlobal('EventSource', FakeEventSource)
    vi.useFakeTimers()
    vi.spyOn(Math, 'random').mockReturnValue(1)
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('puts params on the path and query, and delivers typed events with their id', async () => {
    const onEvent = vi.fn()
    await createHttpTransport().subscribeVaultNoteChanges({ vaultId: 'v 1', classes: 'note' }, { onEvent })
    expect(latest().url).toBe('/api/events/vault-note-changes/v%201?classes=note')
    latest().emit('vault-note-changes', { seq: 1, kind: 'note' }, '0:1')
    expect(onEvent).toHaveBeenCalledWith({ seq: 1, kind: 'note' }, '0:1')
  })

  it('reports a lag frame through onLag', async () => {
    const onLag = vi.fn()
    await createHttpTransport().subscribeVaultNoteChanges({ vaultId: 'v' }, { onEvent: vi.fn(), onLag })
    latest().emit('lag', { skipped: 7 })
    expect(onLag).toHaveBeenCalledWith(7)
  })

  it('reconnects with backoff, resumes from the last seen id, and fires onOpen each time', async () => {
    const onOpen = vi.fn()
    const onError = vi.fn()
    await createHttpTransport().subscribeVaultNoteChanges({ vaultId: 'v' }, { onEvent: vi.fn(), onOpen, onError })
    const first = latest()
    first.open()
    first.emit('vault-note-changes', { seq: 4, kind: 'note' }, '0:4')
    first.fail()
    expect(first.closed).toBe(true)
    expect(onError).toHaveBeenCalledTimes(1)
    expect(FakeEventSource.instances).toHaveLength(1)

    vi.advanceTimersByTime(499)
    expect(FakeEventSource.instances).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(latest().url).toBe('/api/events/vault-note-changes/v?resume=0%3A4')
    latest().open()
    expect(onOpen).toHaveBeenCalledTimes(2)
  })

  it('doubles the delay up to the cap while failures continue', async () => {
    await createHttpTransport().subscribeVaultNoteChanges({ vaultId: 'v' }, { onEvent: vi.fn() })
    const delays: number[] = []
    for (let i = 0; i < 10; i++) {
      const before = FakeEventSource.instances.length
      latest().fail()
      let waited = 0
      while (FakeEventSource.instances.length === before) {
        vi.advanceTimersByTime(100)
        waited += 100
      }
      delays.push(waited)
    }
    expect(delays.slice(0, 4)).toEqual([500, 1000, 2000, 4000])
    expect(Math.max(...delays)).toBe(30000)
  })

  it('starts from args.resume and stops reconnecting once unsubscribed', async () => {
    const unsubscribe = await createHttpTransport().subscribeVaultNoteChanges(
      { vaultId: 'v', resume: '2:9' },
      { onEvent: vi.fn() },
    )
    expect(latest().url).toBe('/api/events/vault-note-changes/v?resume=2%3A9')
    const es = latest()
    unsubscribe()
    expect(es.closed).toBe(true)
    es.fail()
    vi.advanceTimersByTime(60000)
    expect(FakeEventSource.instances).toHaveLength(1)
  })
})

describe('IPC subscribeX', () => {
  beforeEach(() => {
    invokeCalls.length = 0
  })

  it('subscribes over a Channel, routes frames, and unsubscribes by id', async () => {
    setInvokeResult(42)
    const onEvent = vi.fn()
    const onLag = vi.fn()
    const onOpen = vi.fn()
    const unsubscribe = await createIpcTransport().subscribeVaultNoteChanges(
      { vaultId: 'v', resume: '0:3' },
      { onEvent, onLag, onOpen },
    )

    expect(invokeCalls[0].command).toBe('vault_note_changes_subscribe')
    const args = invokeCalls[0].args as Record<string, unknown>
    expect(args).toMatchObject({ vaultId: 'v', classes: null, resume: '0:3' })
    expect(onOpen).toHaveBeenCalledTimes(1)

    const channel = args.channel as Channel<unknown>
    expect(channel).toBeInstanceOf(Channel)
    channel.onmessage({ kind: 'event', id: '0:4', data: { seq: 4, kind: 'note' } })
    channel.onmessage({ kind: 'lag', skipped: 3 })
    expect(onEvent).toHaveBeenCalledWith({ seq: 4, kind: 'note' }, '0:4')
    expect(onLag).toHaveBeenCalledWith(3)

    unsubscribe()
    expect(invokeCalls[1]).toEqual({ command: 'vault_note_changes_unsubscribe', args: { id: 42 } })
  })

  it('sends no args for a parameterless op', async () => {
    setInvokeResult(1)
    await createIpcTransport().subscribeGraphUpdated({}, { onEvent: vi.fn() })
    expect(invokeCalls[0].command).toBe('graph_updated_subscribe')
    expect(Object.keys(invokeCalls[0].args ?? {})).toEqual(['channel'])
  })
})
