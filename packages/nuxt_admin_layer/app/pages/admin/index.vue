<script setup lang="ts">
definePageMeta({ layout: 'admin' })

// Auto-imported from consuming app
const { adminEntities } = useAdminRegistry()
const transport = useTransport()
const projectId = useAdminProjectId()

const counts = ref<Record<string, number>>({})
const loading = ref(true)

onMounted(async () => {
  try {
    // Fetch counts by listing each entity type
    const results = await Promise.allSettled(
      adminEntities.map(async (entity) => {
        const method = entity.listMethod as keyof typeof transport
        const fn = transport[method] as (projectId?: string) => Promise<unknown[]>
        const items = await fn(projectId.value)
        return { key: entity.plural, count: items.length }
      }),
    )
    for (const result of results) {
      if (result.status === 'fulfilled') {
        counts.value[result.value.key] = result.value.count
      }
    }
  } catch {
    // Counts are non-critical
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="p-6 overflow-y-auto h-full">
    <h1 class="text-2xl font-bold text-(--ui-text) mb-6">Admin Dashboard</h1>

    <div v-if="loading" class="text-(--ui-text-muted) text-sm">Loading...</div>

    <div v-else class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
      <NuxtLink
        v-for="entity in adminEntities"
        :key="entity.key"
        :to="`/admin/${entity.plural}`"
        class="block p-4 rounded-lg border border-(--ui-border) bg-(--ui-bg-elevated) hover:bg-(--ui-bg-accented) transition-colors"
      >
        <div class="text-sm text-(--ui-text-muted)">{{ entity.pluralLabel }}</div>
        <div class="text-2xl font-semibold text-(--ui-text) mt-1">
          {{ counts[entity.plural] ?? '—' }}
        </div>
      </NuxtLink>
    </div>
  </div>
</template>
