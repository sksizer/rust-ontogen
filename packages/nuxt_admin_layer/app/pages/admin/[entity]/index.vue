<script setup lang="ts">
definePageMeta({ layout: 'admin' })

const route = useRoute()
const router = useRouter()
const entityPlural = computed(() => route.params.entity as string)

const { adminEntityByPlural } = useAdminRegistry()
const entityConfig = computed(() => adminEntityByPlural[entityPlural.value])

const { items, loading, error, fields, fetchList, getEntityId } = useAdminEntity(entityPlural)

const tableFields = computed(() => fields.value.filter((f) => f.showInTable))

onMounted(() => {
  fetchList()
})

function navigateToDetail(item: Record<string, unknown>) {
  const id = getEntityId(item)
  router.push(`/admin/${entityPlural.value}/${encodeURIComponent(String(id))}`)
}

function navigateToCreate() {
  router.push(`/admin/${entityPlural.value}/new`)
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Header -->
    <div
      class="flex items-center justify-between px-6 py-3 border-b border-(--ui-border) bg-(--ui-bg-elevated)"
    >
      <h1 class="text-lg font-semibold text-(--ui-text)">
        {{ entityConfig?.pluralLabel ?? entityPlural }}
      </h1>
      <button
        class="px-3 py-1.5 text-sm rounded-md bg-(--ui-bg-inverted) text-(--ui-text-inverted) hover:opacity-90 transition-opacity"
        @click="navigateToCreate"
      >
        Create {{ entityConfig?.label }}
      </button>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6">
      <div v-if="loading" class="text-(--ui-text-muted) text-sm">Loading...</div>
      <div v-else-if="error" class="text-red-600 text-sm">{{ error }}</div>
      <div v-else-if="items.length === 0" class="text-(--ui-text-muted) text-sm">
        No {{ entityConfig?.pluralLabel?.toLowerCase() }} found.
      </div>

      <table v-else class="w-full text-sm">
        <thead>
          <tr class="border-b border-(--ui-border)">
            <th
              v-for="field in tableFields"
              :key="field.key"
              class="text-left py-2 px-3 text-(--ui-text-muted) font-medium"
            >
              {{ field.label }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="item in items"
            :key="String(getEntityId(item))"
            class="border-b border-(--ui-border) cursor-pointer hover:bg-(--ui-bg-elevated) transition-colors"
            @click="navigateToDetail(item)"
          >
            <td v-for="field in tableFields" :key="field.key" class="py-2 px-3 text-(--ui-text)">
              <template v-if="field.type === 'relation' && field.relationTo && item[field.key]">
                <span class="text-blue-600 underline">{{ item[field.key] }}</span>
              </template>
              <template v-else>
                {{ formatFieldValue(item, field.key) ?? '—' }}
              </template>
            </td>
          </tr>
        </tbody>
      </table>

      <div v-if="items.length > 0" class="mt-3 text-xs text-(--ui-text-muted)">
        {{ items.length }} {{ items.length === 1 ? 'item' : 'items' }}
      </div>
    </div>
  </div>
</template>
