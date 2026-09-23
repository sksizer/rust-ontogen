import { inject, type InjectionKey } from 'vue'

/** A link from an admin detail page out to the host's own page for that row. */
export interface AdminDetailLink {
  to: string
  label: string
}

/**
 * Provided by the host (`provide(ADMIN_DETAIL_LINK, fn)` in a plugin) to add
 * a link from an entity's admin detail page to its feature page. Return null
 * for an entity without one.
 */
export const ADMIN_DETAIL_LINK: InjectionKey<
  (entity: string, id: string) => AdminDetailLink | null
> = Symbol('ontogen-admin-detail-link')

const none = () => null

export function useAdminDetailLink() {
  return inject(ADMIN_DETAIL_LINK, none)
}
