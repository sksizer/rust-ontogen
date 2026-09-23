// The paginated-envelope rules AdminRelationTables.vue and admin/index.vue
// delegate to, pinned on their own; admin-relation-tables.test.ts and
// admin-dashboard.test.ts cover the component wiring.
import type { AdminEntityConfig } from '@ontogen/admin-types'
import { describe, expect, test } from 'vitest'

import { countFromListResult, fetchAllItems } from '../app/utils/adminListAll'

describe('countFromListResult', () => {
  test('a plain array counts by length', () => {
    expect(countFromListResult([1, 2, 3])).toBe(3)
  })

  test('a paginated envelope counts by total, not the current page size', () => {
    expect(countFromListResult({ items: [1, 2], total: 260 })).toBe(260)
  })
})

type FetchAllConfig = Pick<AdminEntityConfig, 'paginated' | 'maxLimit' | 'defaultLimit' | 'listHasQuery'>

describe('fetchAllItems', () => {
  const nonPaginated: FetchAllConfig = {
    paginated: false,
  }
  const paginated: FetchAllConfig = {
    paginated: true,
    defaultLimit: 50,
    maxLimit: 200,
  }

  test('a non-paginated entity is fetched with a single no-arg call', async () => {
    const calls: unknown[][] = []
    const items = await fetchAllItems(nonPaginated, async (...args) => {
      calls.push(args)
      return [{ id: 'a' }, { id: 'b' }]
    })
    expect(items).toEqual([{ id: 'a' }, { id: 'b' }])
    expect(calls).toEqual([[]])
  })

  test('a paginated entity pages through with maxLimit until every item is collected', async () => {
    const calls: unknown[][] = []
    const all = Array.from({ length: 130 }, (_, i) => ({ id: `item-${i}` }))
    const items = await fetchAllItems(paginated, async (...args) => {
      calls.push(args)
      const [limit, offset] = args as [number, number]
      return { items: all.slice(offset, offset + limit), total: all.length }
    })
    expect(items).toHaveLength(130)
    expect(items[0]).toEqual({ id: 'item-0' })
    expect(items[129]).toEqual({ id: 'item-129' })
    // maxLimit (200) covers the whole 130-row source in a single page.
    expect(calls).toEqual([[200, 0]])
  })

  test('a paginated entity smaller than one page still stops after the short page', async () => {
    const all = [{ id: 'a' }, { id: 'b' }]
    const items = await fetchAllItems(
      { paginated: true, defaultLimit: 50 },
      async () => ({ items: all, total: 2 }),
    )
    expect(items).toEqual(all)
  })

  test('listHasQuery true: calls the three-arg (query, limit, offset) shape', async () => {
    const calls: unknown[][] = []
    const all = [{ id: 'a' }, { id: 'b' }]
    const items = await fetchAllItems({ ...paginated, listHasQuery: true }, async (...args) => {
      calls.push(args)
      return { items: all, total: all.length }
    })
    expect(items).toEqual(all)
    expect(calls).toEqual([[undefined, 200, 0]])
  })

  test('a total that keeps moving the goalpost still terminates, via the page-count ceiling', async () => {
    // A misbehaving fn: every page is full (never `items.length < limit`)
    // and `total` keeps growing just ahead of what's been collected (e.g.
    // concurrent inserts during the fetch), so `acc.length >= total` is
    // never satisfied either. Only the ceiling — fixed from the first
    // response's total (1010 at limit 10 -> ceil(1010/10)+1 = 102 pages) —
    // stops this.
    let calls = 0
    const items = await fetchAllItems({ paginated: true, defaultLimit: 10 }, async () => {
      calls++
      return { items: Array.from({ length: 10 }, (_, i) => ({ id: `${calls}-${i}` })), total: 1_000 + calls * 10 }
    })
    expect(calls).toBe(102)
    expect(items).toHaveLength(1_020)
  })

  test('a non-finite total falls back to the flat 1000-request cap', async () => {
    let calls = 0
    const items = await fetchAllItems({ paginated: true, defaultLimit: 10 }, async () => {
      calls++
      return { items: Array.from({ length: 10 }, (_, i) => ({ id: `${calls}-${i}` })), total: Number.POSITIVE_INFINITY }
    })
    expect(calls).toBe(1000)
    expect(items).toHaveLength(10_000)
  })
})
