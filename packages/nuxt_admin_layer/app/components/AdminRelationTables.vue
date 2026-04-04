<script setup lang="ts">
import type { AdminFieldDef } from '../../admin-fields'

type EntityRecord = Record<string, unknown>

const props = defineProps<{
  entityKey: string
  entityId: string
  fields: AdminFieldDef[]
}>()

// Find entities that reference this entity type via relation fields
interface InverseRelation {
  sourceEntityKey: string
  sourceEntityPlural: string
  sourceEntityLabel: string
  sourceFieldKey: string
  sourceFieldLabel: string
  items: EntityRecord[]
  loading: boolean
}

const transport = useTransport()
const { adminEntityMap, adminFieldDefs } = useAdminRegistry()

const inverseRelations = ref<InverseRelation[]>([])

onMounted(async () => {
  const relations: InverseRelation[] = []

  // Search all entity field defs for fields that point to our entity type
  for (const [entityKey, entityFields] of Object.entries(adminFieldDefs)) {
    if (entityKey === props.entityKey) continue
    const entityConfig = adminEntityMap[entityKey]
    if (!entityConfig) continue

    for (const field of entityFields) {
      if (
        (field.type === 'relation' || field.type === 'relation-array') &&
        field.relationTo === props.entityKey
      ) {
        relations.push({
          sourceEntityKey: entityKey,
          sourceEntityPlural: entityConfig.plural,
          sourceEntityLabel: entityConfig.pluralLabel,
          sourceFieldKey: field.key,
          sourceFieldLabel: field.label,
          items: [],
          loading: true,
        })
      }
    }
  }

  inverseRelations.value = relations

  // Fetch items for each inverse relation in parallel
  await Promise.all(
    inverseRelations.value.map(async (rel) => {
      try {
        const config = adminEntityMap[rel.sourceEntityKey]
        if (!config) return
        const method = config.listMethod as keyof typeof transport
        const fn = transport[method] as (...args: unknown[]) => Promise<EntityRecord[]>
        const allItems = await fn()
        // Filter to items that reference our entity
        rel.items = allItems.filter((item) => {
          const val = item[rel.sourceFieldKey]
          if (Array.isArray(val)) return val.includes(props.entityId)
          return String(val) === props.entityId
        })
      } catch {
        // Ignore fetch errors for inverse relations
      } finally {
        rel.loading = false
      }
    }),
  )
})

function getDisplayField(entityKey: string): string {
  const fields = adminFieldDefs[entityKey]
  if (!fields) return 'id'
  const nameField = fields.find((f) => f.key === 'name' || f.key === 'title')
  return nameField?.key ?? 'id'
}

function getEntityRoute(entityKey: string, item: EntityRecord): string {
  const config = adminEntityMap[entityKey]
  if (!config) return '#'
  const id = item.id ?? item.contract_id ?? ''
  return `/admin/${config.plural}/${encodeURIComponent(String(id))}`
}
</script>

<template>
  <div>
    <template v-for="rel in inverseRelations" :key="`${rel.sourceEntityKey}-${rel.sourceFieldKey}`">
      <div v-if="rel.items.length > 0 || rel.loading" class="mt-6">
        <h3 class="text-sm font-medium text-(--ui-text-muted) mb-2">
          {{ rel.sourceEntityLabel }} (via {{ rel.sourceFieldLabel }})
        </h3>

        <div v-if="rel.loading" class="text-xs text-(--ui-text-muted)">Loading...</div>

        <table
          v-else-if="rel.items.length > 0"
          class="w-full text-sm border border-(--ui-border) rounded"
        >
          <thead>
            <tr class="border-b border-(--ui-border) bg-(--ui-bg-elevated)">
              <th class="text-left py-1.5 px-3 text-(--ui-text-muted) font-medium">ID</th>
              <th class="text-left py-1.5 px-3 text-(--ui-text-muted) font-medium">
                {{ getDisplayField(rel.sourceEntityKey) === 'id' ? 'Details' : 'Name' }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="item in rel.items"
              :key="String(item.id)"
              class="border-b border-(--ui-border) last:border-b-0"
            >
              <td class="py-1.5 px-3">
                <NuxtLink
                  :to="getEntityRoute(rel.sourceEntityKey, item)"
                  class="text-blue-600 hover:underline text-sm"
                >
                  {{ item.id }}
                </NuxtLink>
              </td>
              <td class="py-1.5 px-3 text-(--ui-text) text-sm">
                {{ item[getDisplayField(rel.sourceEntityKey)] ?? '—' }}
              </td>
            </tr>
          </tbody>
        </table>

        <div v-else class="text-xs text-(--ui-text-muted)">None</div>
      </div>
    </template>
  </div>
</template>
