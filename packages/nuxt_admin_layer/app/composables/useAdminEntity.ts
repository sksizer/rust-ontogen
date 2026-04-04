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

  const fields = computed<AdminFieldDef[]>(() => {
    if (!config.value) return []
    return adminFieldDefs[config.value.key] ?? []
  })

  async function fetchList() {
    if (!config.value) return
    loading.value = true
    error.value = null
    try {
      const method = config.value.listMethod as keyof typeof transport
      const fn = transport[method] as (...args: unknown[]) => Promise<EntityRecord[]>
      items.value = await fn()
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

  // Re-fetch when the entity type changes
  if (isRef(pluralOrKey)) {
    watch(resolvedKey, () => {
      items.value = []
      currentItem.value = null
      error.value = null
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
    fetchList,
    fetchById,
    createEntity,
    updateEntity,
    deleteEntity,
    getEntityId,
    getDisplayValue,
  }
}
