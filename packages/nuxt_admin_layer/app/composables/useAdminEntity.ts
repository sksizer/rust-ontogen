import { ref, computed, watch, isRef, type Ref } from 'vue'
import type { AdminFieldDef } from '../../admin-fields'

type EntityRecord = Record<string, unknown>

/**
 * Contract: the consuming app must provide these auto-imported composables:
 *
 *   useTransport()        — returns a Transport object with CRUD methods
 *   useAdminProjectId()   — returns Ref<string | undefined> for multi-tenancy
 *   useAdminRegistry()    — returns { adminEntityByPlural, adminFieldDefs }
 *
 * These are resolved via Nuxt auto-imports at runtime.
 */

export interface AdminEntityConfig {
  key: string
  plural: string
  label: string
  pluralLabel: string
  idType: 'string'
  listMethod: string
  getMethod: string
  createMethod: string
  updateMethod: string
  deleteMethod: string
  returnType: string
  createInputType: string
  updateInputType: string
}

export function useAdminEntity(pluralOrKey: string | Ref<string>) {
  // These are auto-imported from the consuming app
  const transport = useTransport()
  const projectId = useAdminProjectId()
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

  const pid = () => projectId.value

  async function fetchList() {
    if (!config.value) return
    loading.value = true
    error.value = null
    try {
      const method = config.value.listMethod as keyof typeof transport
      const fn = transport[method] as unknown as (projectId?: string) => Promise<EntityRecord[]>
      items.value = await fn(pid())
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
      const fn = transport[method] as unknown as (id: string, projectId?: string) => Promise<EntityRecord>
      currentItem.value = await fn(id, pid())
    } catch (e) {
      error.value = String(e)
    } finally {
      loading.value = false
    }
  }

  async function createEntity(input: EntityRecord) {
    if (!config.value) throw new Error('No entity config')
    const method = config.value.createMethod as keyof typeof transport
    const fn = transport[method] as unknown as (
      input: EntityRecord,
      projectId?: string,
    ) => Promise<EntityRecord>
    return await fn(input, pid())
  }

  async function updateEntity(id: string, input: EntityRecord) {
    if (!config.value) throw new Error('No entity config')
    const method = config.value.updateMethod as keyof typeof transport
    const fn = transport[method] as unknown as (
      id: string,
      input: EntityRecord,
      projectId?: string,
    ) => Promise<EntityRecord>
    return await fn(id, input, pid())
  }

  async function deleteEntity(id: string) {
    if (!config.value) throw new Error('No entity config')
    const method = config.value.deleteMethod as keyof typeof transport
    const fn = transport[method] as unknown as (id: string, projectId?: string) => Promise<unknown>
    await fn(id, pid())
  }

  // Re-fetch when the entity type changes (BUG-012)
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
    for (const key of ['name', 'title', 'id', 'contract_id']) {
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
