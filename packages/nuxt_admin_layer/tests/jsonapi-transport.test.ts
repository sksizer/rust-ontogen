/**
 * The generated HTTP transport against a stubbed `fetch`: JSON:API goes on
 * and comes off inside it, and callers see flat entities (wire contract §14).
 * The transport under test is `fixtures/jsonapi-transport.generated.ts`, which
 * the Rust test `ts_jsonapi_transport_fixture_is_current` keeps equal to the
 * generator's output.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { createHttpTransport, JsonApiError } from './fixtures/jsonapi-transport.generated'

const MEDIA_TYPE = 'application/vnd.api+json'

interface Call {
  url: string
  method: string
  headers: Record<string, string>
  body: unknown
}

let calls: Call[] = []
let replies: Response[] = []

function reply(status: number, body?: unknown, statusText = ''): void {
  const text = body === undefined ? null : typeof body === 'string' ? body : JSON.stringify(body)
  replies.push(new Response(text, { status, statusText }))
}

beforeEach(() => {
  calls = []
  replies = []
  vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
    calls.push({
      url,
      method: init.method ?? 'GET',
      headers: init.headers as Record<string, string>,
      body: init.body == null ? undefined : JSON.parse(init.body as string),
    })
    const next = replies.shift()
    if (!next) throw new Error(`no reply queued for ${url}`)
    return next
  })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

const shipIt = {
  type: 'tasks',
  id: 'ship-it',
  attributes: { title: 'Ship it', estimate: null, body: '## Goal\n' },
  relationships: {
    parent: { data: { type: 'tasks', id: 'release' } },
    subtasks: { data: [{ type: 'tasks', id: 'tag-it' }] },
    tags: { data: [{ type: 'tags', id: 'codegen' }, { type: 'tags', id: 'wire' }] },
  },
  links: { self: '/api/tasks/ship-it' },
}

const flatShipIt = {
  id: 'ship-it',
  title: 'Ship it',
  estimate: null,
  body: '## Goal\n',
  parent_id: 'release',
  subtasks: ['tag-it'],
  tags: ['codegen', 'wire'],
}

describe('reads', () => {
  it('flattens a resource: id, attributes, to-one id, to-many ids', async () => {
    reply(200, { jsonapi: { version: '1.1' }, data: shipIt })
    const task = await createHttpTransport().taskGetById('ship it')

    expect(task).toEqual(flatShipIt)
    expect(calls[0]).toMatchObject({ url: '/api/tasks/ship%20it', method: 'GET', body: undefined })
    expect(calls[0]?.headers).toEqual({ Accept: MEDIA_TYPE })
  })

  it('reads absent or null linkage as null and []', async () => {
    reply(200, {
      data: {
        type: 'tasks',
        id: 'orphan',
        attributes: { title: 'Orphan', estimate: 3, body: '' },
        relationships: { parent: { data: null } },
      },
    })
    const task = await createHttpTransport().taskGetById('orphan')

    expect(task).toEqual({
      id: 'orphan',
      title: 'Orphan',
      estimate: 3,
      body: '',
      parent_id: null,
      subtasks: [],
      tags: [],
    })
  })

  it('names the id after the entity id field', async () => {
    reply(200, { data: { type: 'tags', id: 'codegen', attributes: { title: 'Codegen' } } })
    expect(await createHttpTransport().tagGetById('codegen')).toEqual({ slug: 'codegen', title: 'Codegen' })
  })

  it('pages with page[offset] and page[limit] and rebuilds PaginatedResult from meta', async () => {
    reply(200, {
      links: { self: '/api/tasks?page%5Boffset%5D=20&page%5Blimit%5D=10' },
      meta: { total: 45, limit: 10, offset: 20 },
      data: [shipIt],
    })
    const page = await createHttpTransport().taskList(10, 20)

    expect(calls[0]?.url).toBe('/api/tasks?page%5Boffset%5D=20&page%5Blimit%5D=10')
    expect(page).toEqual({ items: [flatShipIt], total: 45, limit: 10, offset: 20 })
  })

  it('sends only the page members that are defined', async () => {
    reply(200, { meta: { total: 0, limit: 20, offset: 0 }, data: [] })
    reply(200, { meta: { total: 0, limit: 5, offset: 0 }, data: [] })
    const transport = createHttpTransport()

    expect(await transport.taskList()).toEqual({ items: [], total: 0, limit: 20, offset: 0 })
    await transport.taskList(5)
    expect(calls.map((c) => c.url)).toEqual(['/api/tasks', '/api/tasks?page%5Blimit%5D=5'])
  })
})

describe('writes', () => {
  it('creates from an unflattened document and flattens the 201 reply', async () => {
    reply(201, { data: shipIt })
    const created = await createHttpTransport().taskCreate({
      id: 'ship-it',
      title: 'Ship it',
      estimate: null,
      parent_id: 'release',
      subtasks: ['tag-it'],
      tags: ['codegen', 'wire'],
      body: '## Goal\n',
    })

    expect(created).toEqual(flatShipIt)
    expect(calls[0]).toMatchObject({ url: '/api/tasks', method: 'POST' })
    expect(calls[0]?.headers).toEqual({ Accept: MEDIA_TYPE, 'Content-Type': MEDIA_TYPE })
    expect(calls[0]?.body).toEqual({
      data: {
        type: 'tasks',
        id: 'ship-it',
        attributes: { title: 'Ship it', estimate: null, body: '## Goal\n' },
        relationships: {
          parent: { data: { type: 'tasks', id: 'release' } },
          subtasks: { data: [{ type: 'tasks', id: 'tag-it' }] },
          tags: { data: [{ type: 'tags', id: 'codegen' }, { type: 'tags', id: 'wire' }] },
        },
      },
    })
  })

  it('leaves an empty create id out so the server derives one', async () => {
    const tag = { tag: { data: { type: 'tags', id: 'push' } } }
    reply(201, { data: { type: 'workout-sets', id: 'derived', attributes: { reps: 5 }, relationships: tag } })
    const created = await createHttpTransport().workoutSetCreate({ id: '', reps: 5, tag_id: 'push' })

    expect(calls[0]?.url).toBe('/api/workout-sets')
    expect(calls[0]?.body).toEqual({ data: { type: 'workout-sets', attributes: { reps: 5 }, relationships: tag } })
    expect(created).toEqual({ id: 'derived', reps: 5, tag_id: 'push' })
  })

  it('patches with the URL id, omits undefined, keeps null, and clears a to-one with data: null', async () => {
    reply(200, { data: shipIt })
    await createHttpTransport().taskUpdate('ship-it', {
      title: undefined,
      estimate: null,
      parent_id: null,
      tags: [],
      subtasks: null,
    })

    expect(calls[0]).toMatchObject({ url: '/api/tasks/ship-it', method: 'PATCH' })
    expect(calls[0]?.headers).toEqual({ Accept: MEDIA_TYPE, 'Content-Type': MEDIA_TYPE })
    expect(calls[0]?.body).toEqual({
      data: {
        type: 'tasks',
        id: 'ship-it',
        attributes: { estimate: null },
        relationships: { parent: { data: null }, tags: { data: [] } },
      },
    })
  })

  it('sends no relationships member for a type without relationships', async () => {
    reply(200, { data: { type: 'tags', id: 'codegen', attributes: { title: 'Code' } } })
    await createHttpTransport().tagUpdate('codegen', { title: 'Code' })

    expect(calls[0]?.body).toEqual({ data: { type: 'tags', id: 'codegen', attributes: { title: 'Code' } } })
  })

  it('deletes and resolves null on 204', async () => {
    reply(204)
    expect(await createHttpTransport().taskDelete('ship-it')).toBeNull()
    expect(calls[0]).toMatchObject({ url: '/api/tasks/ship-it', method: 'DELETE', body: undefined })
    expect(calls[0]?.headers).toEqual({ Accept: MEDIA_TYPE })
  })
})

describe('errors', () => {
  it('throws JsonApiError carrying the status and errors, with the first detail as message', async () => {
    const errors = [{ status: '404', code: 'task_not_found', title: 'Not Found', detail: 'Task not found: nope' }]
    reply(404, { jsonapi: { version: '1.1' }, errors }, 'Not Found')
    const err = await createHttpTransport().taskGetById('nope').catch((e: unknown) => e)

    expect(err).toBeInstanceOf(JsonApiError)
    expect(err).toBeInstanceOf(Error)
    const e = err as JsonApiError
    expect(e.name).toBe('JsonApiError')
    expect(e.status).toBe(404)
    expect(e.errors).toEqual(errors)
    expect(e.message).toBe('Task not found: nope')
    expect(String(e)).toBe('JsonApiError: Task not found: nope')
  })

  it('falls back to the title when the first error has no detail', async () => {
    reply(406, { errors: [{ status: '406', title: 'Not Acceptable' }] })
    const err = (await createHttpTransport().taskList().catch((e: unknown) => e)) as JsonApiError

    expect(err.message).toBe('Not Acceptable')
  })

  it('throws with no errors and the status text when the body is not an error document', async () => {
    reply(502, '<html>Bad Gateway</html>', 'Bad Gateway')
    const err = (await createHttpTransport().taskDelete('x').catch((e: unknown) => e)) as JsonApiError

    expect(err).toBeInstanceOf(JsonApiError)
    expect(err.status).toBe(502)
    expect(err.errors).toEqual([])
    expect(err.message).toBe('Bad Gateway')
  })
})

describe('ops served as custom ops', () => {
  const result = (value: unknown) => ({ jsonapi: { version: '1.1' }, meta: { result: value } })

  it('sends a custom GET its required args in the path and optional ones as opArg, and reads meta.result', async () => {
    reply(200, result(flatShipIt))
    reply(200, result(flatShipIt))
    const transport = createHttpTransport()

    expect(await transport.boardGetSummary('ship it', false)).toEqual(flatShipIt)
    await transport.boardGetSummary('ship-it', null)
    expect(calls[0]).toMatchObject({
      url: '/api/boards/summary/ship%20it?opArg%5Binclude_done%5D=false',
      method: 'GET',
      body: undefined,
    })
    expect(calls[0]?.headers).toEqual({ Accept: MEDIA_TYPE })
    expect(calls[1]?.url).toBe('/api/boards/summary/ship-it')
  })

  it('sends every custom POST arg as meta.args under its Rust name, with no query string', async () => {
    reply(200, result(flatShipIt))
    reply(200, result(3))
    const transport = createHttpTransport()

    expect(await transport.boardArchive('ship-it', null)).toEqual(flatShipIt)
    expect(await transport.boardImport({ ...flatShipIt, id: '' }, true)).toBe(3)
    expect(calls[0]).toMatchObject({ url: '/api/boards/archive', method: 'POST' })
    expect(calls[0]?.headers).toEqual({ Accept: MEDIA_TYPE, 'Content-Type': MEDIA_TYPE })
    expect(calls[0]?.body).toEqual({ meta: { args: { task_id: 'ship-it', reason: null } } })
    expect(calls[1]?.url).toBe('/api/boards/import')
    expect(calls[1]?.body).toEqual({ meta: { args: { input: { ...flatShipIt, id: '' }, dry_run: true } } })
  })

  it('sends no body for a custom POST without args and resolves null on 204', async () => {
    reply(204)
    expect(await createHttpTransport().boardReset()).toBeNull()
    expect(calls[0]).toMatchObject({ url: '/api/boards/reset', method: 'POST', body: undefined })
    expect(calls[0]?.headers).toEqual({ Accept: MEDIA_TYPE })
  })

  it('serves CRUD with no entity behind it as custom ops', async () => {
    const page = { items: [flatShipIt], total: 1, limit: 5, offset: 0 }
    reply(200, result(page))
    reply(200, result(flatShipIt))
    reply(200, result(flatShipIt))
    reply(204)
    const transport = createHttpTransport()

    expect(await transport.boardList(5)).toEqual(page)
    expect(await transport.boardCreate({ ...flatShipIt })).toEqual(flatShipIt)
    expect(await transport.boardUpdate('b 1', { title: 'Ship it' })).toEqual(flatShipIt)
    expect(await transport.boardDelete('b 1')).toBeNull()
    expect(calls.map((c) => [c.method, c.url])).toEqual([
      ['GET', '/api/boards?opArg%5Blimit%5D=5'],
      ['POST', '/api/boards'],
      ['PATCH', '/api/boards/b%201'],
      ['DELETE', '/api/boards/b%201'],
    ])
    expect(calls[1]?.body).toEqual({ meta: { args: { input: flatShipIt } } })
    expect(calls[2]?.body).toEqual({ meta: { args: { input: { title: 'Ship it' } } } })
    expect(calls[3]?.body).toBeUndefined()
  })

  it('calls junction ops at their nested routes', async () => {
    const page = { items: [{ slug: 'codegen', title: 'Codegen' }], total: 1, limit: 10, offset: 0 }
    reply(200, result(page))
    reply(204)
    reply(204)
    const transport = createHttpTransport()

    expect(await transport.boardListTags('b1', 10, 0)).toEqual(page)
    expect(await transport.boardAddTag('b1', 'codegen')).toBeNull()
    expect(await transport.boardRemoveTag('b1', 'code gen')).toBeNull()
    expect(calls.map((c) => [c.method, c.url])).toEqual([
      ['GET', '/api/boards/b1/tags?opArg%5Blimit%5D=10&opArg%5Boffset%5D=0'],
      ['POST', '/api/boards/b1/tags'],
      ['DELETE', '/api/boards/b1/tags/code%20gen'],
    ])
    expect(calls[1]?.body).toEqual({ meta: { args: { tag_id: 'codegen' } } })
    expect(calls[2]?.body).toBeUndefined()
  })

  it('throws JsonApiError for an op error document', async () => {
    reply(404, { errors: [{ status: '404', code: 'not_found', detail: 'no board b9' }] }, 'Not Found')
    const err = (await createHttpTransport().boardGetById('b9').catch((e: unknown) => e)) as JsonApiError

    expect(err).toBeInstanceOf(JsonApiError)
    expect(err.status).toBe(404)
    expect(err.message).toBe('no board b9')
  })
})

describe('subscriptions', () => {
  class FakeEventSource {
    static latest: FakeEventSource | null = null
    readonly url: string
    onopen: (() => void) | null = null
    onerror: ((err: Event) => void) | null = null
    private listeners = new Map<string, ((event: MessageEvent) => void)[]>()

    constructor(url: string) {
      this.url = url
      FakeEventSource.latest = this
    }

    addEventListener(name: string, fn: (event: MessageEvent) => void): void {
      this.listeners.set(name, [...(this.listeners.get(name) ?? []), fn])
    }

    close(): void {}

    emit(name: string, data: unknown, lastEventId = ''): void {
      const event = { data: JSON.stringify(data), lastEventId } as MessageEvent
      for (const fn of this.listeners.get(name) ?? []) fn(event)
    }
  }

  it('flattens a frame whose item is an entity', async () => {
    vi.stubGlobal('EventSource', FakeEventSource)
    const onEvent = vi.fn()
    await createHttpTransport().subscribeTaskChanges({ resume: '0:6' }, { onEvent })
    const { links: _links, ...frame } = shipIt

    expect(FakeEventSource.latest?.url).toBe('/api/events/task-changes?resume=0%3A6')
    FakeEventSource.latest?.emit('task-changes', frame, '0:7')
    expect(onEvent).toHaveBeenCalledWith(flatShipIt, '0:7')
  })
})
