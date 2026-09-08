import type { AdminEntityConfig } from '@ontogen/admin-types'

/**
 * The arguments that ask a paginated list method for one page. The
 * transport puts a query struct first when the list takes one
 * (`listQuery`), and the page goes after it.
 */
export function pageArgs(config: AdminEntityConfig, limit: number, offset: number): unknown[] {
  return config.listQuery ? [undefined, limit, offset] : [limit, offset]
}
