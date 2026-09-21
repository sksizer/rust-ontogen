import { ref, computed, watch, isRef, type Ref } from 'vue'
import type { AdminFieldDef, AdminEntityConfig } from '@ontogen/admin-types'

type EntityRecord = Record<string, unknown>

/**
 * Contract: the consuming app must provide this auto-imported composable:
 *
 *   useTransport() - returns a Transport object with CRUD methods
 *
 * The admin registry is provided automatically by the layer's Nuxt module
 * via the #admin-registry alias pointing to app/admin/generated/admin-registry.ts.
 */

export function useAdminEntity(pluralOrKey: string | Ref<string>) {
  const transport = useTransport()
  const { adminEntityByPlural, adminFieldDefs } = useAdminRegistry()

  const resolvedKey = computed(() => (isRef(pluralOrKey) ? pluralOrKey.value : pluralOrKey))

  const config = computed<AdminEntityConfig | undefined>(() => {
    return adminEntityByPlural[resolvedKey.value]
  })

  const items = ref<EntityRecord[]>([])
  const currentItem = ref<EntityRecord | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  /** Current page number (1-based). Only relevant when entity is paginated. */
  const page = ref(1)
  /** Number of items per page. Defaults to entity's configured defaultLimit or 50. */
  const limit = ref(config.value?.defaultLimit ?? 50)
  /** Total number of items available server-side. Only set for paginated entities. */
  const total = ref(0)
  /** Total number of pages based on current limit and total count. */
  const totalPages = computed(() => Math.ceil(total.value / limit.value) || 1)

  const fields = computed<AdminFieldDef[]>(() => {
    if (!config.value) return []
    return adminFieldDefs[config.value.key] ?? []
  })

  /**
   * Monotonically increasing id for the in-flight fetchList request.
   *
   * nextPage/prevPage/goToPage each fire fetchList without awaiting the
   * previous call, so two requests can be in flight together. Whichever
   * response resolves first is not necessarily the one for the page the UI
   * is now showing; each call captures its own id and only applies its
   * result if it is still the latest one when its response lands.
   */
  let fetchListRequestId = 0

  /**
   * Fetch the entity list from the transport layer.
   *
   * For paginated entities, passes limit/offset and unwraps the
   * PaginatedResult envelope. Non-paginated entities receive the
   * array directly.
   */
  async function fetchList() {
    if (!config.value) return
    const requestId = ++fetchListRequestId
    loading.value = true
    error.value = null
    try {
      const method = config.value.listMethod as keyof typeof transport
      if (config.value.paginated) {
        const offset = (page.value - 1) * limit.value
        // The Transport union spans CRUD + event-subscription method shapes;
        // narrowing to the list-call signature requires going through
        // `unknown` because the union has signatures (e.g. event
        // subscriptions returning `Promise<() => void>`) that don't overlap
        // with this paginated return type.
        const fn = transport[method] as unknown as (...args: unknown[]) => Promise<{ items: EntityRecord[]; total: number }>
        // The generated list method's call shape depends on whether the
        // underlying Rust fn has a query-struct param (see
        // src/clients/generators/transport.rs's OpKind::List branch, and
        // src/clients/generators/admin.rs, which records this on the entity
        // config as listHasQuery): (query?, limit?, offset?) when true,
        // (limit?, offset?) when false/absent. Guessing the shape silently
        // shifts every argument, so this is not optional.
        const result = config.value.listHasQuery
          ? await fn(undefined, limit.value, offset)
          : await fn(limit.value, offset)
        if (requestId !== fetchListRequestId) return // a newer fetchList has since started; drop this response
        // The total may have shrunk since `page` was chosen (e.g. rows
        // deleted elsewhere). Landing past the last real page returns an
        // empty page and strands the UI there with no control to leave it
        // (see index.vue's AdminPagination visibility), so clamp and retry.
        const newTotalPages = Math.ceil(result.total / limit.value) || 1
        if (page.value > newTotalPages && page.value > 1) {
          page.value = newTotalPages
          return fetchList()
        }
        items.value = result.items
        total.value = result.total
      } else {
        const fn = transport[method] as unknown as (...args: unknown[]) => Promise<EntityRecord[]>
        const result = await fn()
        if (requestId !== fetchListRequestId) return
        items.value = result
      }
    } catch (e) {
      if (requestId !== fetchListRequestId) return
      error.value = String(e)
    } finally {
      if (requestId === fetchListRequestId) loading.value = false
    }
  }

  async function fetchById(id: string) {
    if (!config.value) return
    loading.value = true
    error.value = null
    try {
      const method = config.value.getMethod as keyof typeof transport
      const fn = transport[method] as (...args: unknown[]) => Promise<EntityRecord>
      currentItem.value = await fn(id)
    } catch (e) {
      error.value = String(e)
    } finally {
      loading.value = false
    }
  }

  async function createEntity(input: EntityRecord) {
    if (!config.value) throw new Error('No entity config')
    const method = config.value.createMethod as keyof typeof transport
    const fn = transport[method] as (...args: unknown[]) => Promise<EntityRecord>
    return await fn(input)
  }

  async function updateEntity(id: string, input: EntityRecord) {
    if (!config.value) throw new Error('No entity config')
    const method = config.value.updateMethod as keyof typeof transport
    const fn = transport[method] as (...args: unknown[]) => Promise<EntityRecord>
    return await fn(id, input)
  }

  async function deleteEntity(id: string) {
    if (!config.value) throw new Error('No entity config')
    const method = config.value.deleteMethod as keyof typeof transport
    const fn = transport[method] as (...args: unknown[]) => Promise<unknown>
    await fn(id)
  }

  /** Advance to the next page (no-op if already on the last page). */
  function nextPage() {
    if (page.value < totalPages.value) { page.value++; fetchList() }
  }

  /** Go back to the previous page (no-op if already on page 1). */
  function prevPage() {
    if (page.value > 1) { page.value--; fetchList() }
  }

  /** Jump to a specific page number, clamped to valid range. */
  function goToPage(p: number) {
    page.value = Math.max(1, Math.min(p, totalPages.value))
    fetchList()
  }

  /** Change the page size and reset to the first page. */
  function setPageSize(size: number) {
    limit.value = size
    page.value = 1
    fetchList()
  }

  // Re-fetch when the entity type changes
  if (isRef(pluralOrKey)) {
    watch(resolvedKey, () => {
      items.value = []
      currentItem.value = null
      error.value = null
      page.value = 1
      total.value = 0
      limit.value = config.value?.defaultLimit ?? 50
      fetchList()
    })
  }

  function getEntityId(item: EntityRecord): string {
    if (!config.value) return ''
    return String(item.id)
  }

  function getDisplayValue(item: EntityRecord): string {
    if (!config.value) return ''
    for (const key of ['name', 'title', 'id']) {
      if (item[key] != null && String(item[key]).length > 0) {
        return String(item[key])
      }
    }
    return String(getEntityId(item))
  }

  return {
    config,
    items,
    currentItem,
    loading,
    error,
    fields,
    page,
    limit,
    total,
    totalPages,
    fetchList,
    fetchById,
    createEntity,
    updateEntity,
    deleteEntity,
    getEntityId,
    getDisplayValue,
    nextPage,
    prevPage,
    goToPage,
    setPageSize,
  }
}
