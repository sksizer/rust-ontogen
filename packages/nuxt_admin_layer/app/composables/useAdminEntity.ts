import { ref, computed, watch, isRef, type Ref } from 'vue'
import type { AdminFieldDef, AdminEntityConfig } from '@ontogen/admin-types'

type EntityRecord = Record<string, unknown>

/**
 * Contract: the consuming app must provide this auto-imported composable:
 *
 *   useTransport() — returns a Transport object with CRUD methods
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
   * Fetch the entity list from the transport layer.
   *
   * For paginated entities, passes limit/offset and unwraps the
   * PaginatedResult envelope. Non-paginated entities receive the
   * array directly.
   */
  async function fetchList() {
    if (!config.value) return
    loading.value = true
    error.value = null
    try {
      const method = config.value.listMethod as keyof typeof transport
      if (config.value.paginated) {
        const offset = (page.value - 1) * limit.value
        const fn = transport[method] as (...args: unknown[]) => Promise<{ items: EntityRecord[]; total: number }>
        const result = await fn(undefined, limit.value, offset)
        items.value = result.items
        total.value = result.total
      } else {
        const fn = transport[method] as (...args: unknown[]) => Promise<EntityRecord[]>
        items.value = await fn()
      }
    } catch (e) {
      error.value = String(e)
    } finally {
      loading.value = false
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
