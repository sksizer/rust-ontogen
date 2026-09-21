// useAdminEntity's fetchList: the paginated-listing call shape, request
// sequencing, and page-clamping rules, pinned directly against the composable
// (no .vue mount needed — the defect lives entirely in fetchList's own logic).
import type { AdminEntityConfig } from '@ontogen/admin-types'
import { beforeEach, describe, expect, test } from 'vitest'

import { useAdminEntity } from '../app/composables/useAdminEntity'

const widget: AdminEntityConfig = {
  key: 'widget',
  plural: 'widgets',
  label: 'Widget',
  pluralLabel: 'Widgets',
  idType: 'string',
  listMethod: 'widgetList',
  getMethod: 'widgetGetById',
  createMethod: 'widgetCreate',
  updateMethod: 'widgetUpdate',
  deleteMethod: 'widgetDelete',
  returnType: 'Widget',
  createInputType: 'CreateWidgetInput',
  updateInputType: 'UpdateWidgetInput',
  paginated: true,
  defaultLimit: 50,
  maxLimit: 200,
  fields: [],
}

// Same shape as `widget`, but its (fixture) list fn takes a leading
// query-struct param — the other call shape transport.rs's OpKind::List
// can generate.
const widgetWithQuery: AdminEntityConfig = { ...widget, listHasQuery: true }

function stubRegistryFor(
  entity: AdminEntityConfig,
  transport: Record<string, (...args: unknown[]) => Promise<unknown>>,
) {
  Object.assign(globalThis, {
    useTransport: () => transport,
    useAdminRegistry: () => ({
      adminEntities: [entity],
      adminEntityMap: { [entity.key]: entity },
      adminEntityByPlural: { [entity.plural]: entity },
      adminFieldDefs: { [entity.key]: [] },
    }),
  })
}

function stubRegistry(transport: Record<string, (...args: unknown[]) => Promise<unknown>>) {
  stubRegistryFor(widget, transport)
}

beforeEach(() => {
  // useAdminEntity() reads useTransport/useAdminRegistry once at call time,
  // so each test installs its own stub before constructing the composable.
  Object.assign(globalThis, { useTransport: undefined, useAdminRegistry: undefined })
})

describe('fetchList paginated call shape', () => {
  test('listHasQuery false/absent: calls the two-arg (limit, offset) shape, not (undefined, limit, offset)', async () => {
    const calls: unknown[][] = []
    stubRegistry({
      widgetList: async (...args: unknown[]) => {
        calls.push(args)
        return { items: [{ id: 'a' }], total: 1 }
      },
    })

    const { fetchList } = useAdminEntity('widgets')
    await fetchList()

    expect(calls).toEqual([[50, 0]])
  })

  test('listHasQuery true: calls the three-arg (query, limit, offset) shape', async () => {
    const calls: unknown[][] = []
    stubRegistryFor(widgetWithQuery, {
      widgetList: async (...args: unknown[]) => {
        calls.push(args)
        return { items: [{ id: 'a' }], total: 1 }
      },
    })

    const { fetchList } = useAdminEntity('widgets')
    await fetchList()

    expect(calls).toEqual([[undefined, 50, 0]])
  })
})

describe('fetchList request sequencing', () => {
  test('a stale in-flight response is dropped once a newer fetchList has resolved', async () => {
    let resolveFirst!: (value: unknown) => void
    let callIndex = 0
    stubRegistry({
      // total: 200 keeps both page 1 and page 2 within totalPages (4 at the
      // default limit of 50), so this exercises sequencing only, not the
      // separate page-clamp behavior covered below.
      widgetList: async () => {
        callIndex++
        if (callIndex === 1) {
          return new Promise((resolve) => {
            resolveFirst = resolve
          })
        }
        return { items: [{ id: 'second' }], total: 200 }
      },
    })

    const { fetchList, items, loading, page } = useAdminEntity('widgets')

    const firstCall = fetchList() // page 1, left in flight
    page.value = 2
    await fetchList() // page 2, resolves immediately

    expect(items.value[0]?.id).toBe('second')
    expect(loading.value).toBe(false)

    resolveFirst({ items: [{ id: 'first' }], total: 200 }) // the stale page-1 response arrives late
    await firstCall
    await Promise.resolve()

    expect(items.value[0]?.id).toBe('second')
    expect(loading.value).toBe(false)
  })
})

describe('fetchList page clamp', () => {
  test('lands on the last real page instead of stranding on an empty page past totalPages', async () => {
    let call = 0
    const calls: unknown[][] = []
    stubRegistry({
      widgetList: async (...args: unknown[]) => {
        calls.push(args)
        call++
        // First call: caller asked for page 6 (offset 250) but the server-side
        // total has shrunk to 160 (totalPages 4) since that page number was chosen.
        if (call === 1) return { items: [], total: 160 }
        // Second call: the composable should retry at the clamped page (offset 150).
        return { items: [{ id: 'p150' }], total: 160 }
      },
    })

    const { fetchList, items, page, total } = useAdminEntity('widgets')
    page.value = 6
    await fetchList()

    expect(page.value).toBe(4)
    expect(items.value).toEqual([{ id: 'p150' }])
    expect(total.value).toBe(160)
    expect(calls).toEqual([
      [50, 250],
      [50, 150],
    ])
  })
})
