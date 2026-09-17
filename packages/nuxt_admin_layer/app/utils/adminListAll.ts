import type { AdminEntityConfig } from '@ontogen/admin-types'

export interface PaginatedListResult<T> {
  items: T[]
  total: number
}

/** What a list method returns: the full array, or one page of a paginated entity. */
export type ListResult<T> = T[] | PaginatedListResult<T>

function isPaginatedResult<T>(result: ListResult<T>): result is PaginatedListResult<T> {
  return !Array.isArray(result)
}

/** The row count a list result represents, whichever shape it is. */
export function countFromListResult<T>(result: ListResult<T>): number {
  return isPaginatedResult(result) ? result.total : result.length
}

/**
 * Every item from a list method, whichever shape it returns.
 *
 * A non-paginated entity's list method returns the full array from one call.
 * A `config.paginated` entity's list method returns one page as a
 * `{ items, total }` envelope; this pages through with
 * `limit = config.maxLimit ?? config.defaultLimit ?? 50` until every item has
 * been collected. `config.listHasQuery` picks the call shape the same way
 * useAdminEntity.fetchList does: `fn(undefined, limit, offset)` when the
 * underlying Rust list fn takes a query-struct param, `fn(limit, offset)`
 * otherwise (see src/clients/generators/transport.rs's OpKind::List branch).
 *
 * A defensive page-count ceiling guards against turning a misbehaving fn
 * into an infinite loop: one whose `total` keeps moving the goalpost (e.g.
 * concurrent inserts during the fetch) never satisfies `acc.length >= total`,
 * and one that always returns a full page never satisfies
 * `items.length < limit`. The ceiling is `ceil(total / limit) + 1` pages —
 * one page later than a well-behaved `total` should need — capped at 1000
 * requests regardless, so even a bogus or non-finite `total` can't run away.
 */
export async function fetchAllItems<T>(
  config: Pick<AdminEntityConfig, 'paginated' | 'maxLimit' | 'defaultLimit' | 'listHasQuery'>,
  fn: (...args: unknown[]) => Promise<ListResult<T>>,
): Promise<T[]> {
  if (!config.paginated) {
    const result = await fn()
    return isPaginatedResult(result) ? result.items : result
  }

  const limit = config.maxLimit ?? config.defaultLimit ?? 50
  const acc: T[] = []
  let offset = 0
  let pagesFetched = 0
  let maxPages = 1000
  for (;;) {
    const page = config.listHasQuery ? await fn(undefined, limit, offset) : await fn(limit, offset)
    const items = isPaginatedResult(page) ? page.items : page
    const total = isPaginatedResult(page) ? page.total : items.length
    acc.push(...items)
    offset += items.length
    pagesFetched++
    if (pagesFetched === 1) {
      const byTotal = Number.isFinite(total) && total > 0 ? Math.ceil(total / limit) + 1 : 1000
      maxPages = Math.min(byTotal, 1000)
    }
    if (items.length < limit || acc.length >= total || pagesFetched >= maxPages) break
  }
  return acc
}
