<script setup lang="ts">
definePageMeta({ layout: 'admin' })

const route = useRoute()
const router = useRouter()
const entityPlural = computed(() => route.params.entity as string)
const entityId = computed(() => route.params.id as string)

const { adminEntityByPlural } = useAdminRegistry()
const entityConfig = computed(() => adminEntityByPlural[entityPlural.value])

const { currentItem, fields, fetchById, updateEntity, getEntityId } = useAdminEntity(
  entityPlural.value,
)

const formFields = computed(() =>
  fields.value.filter((f) => f.showInForm && !f.readOnly && !f.isId),
)

const formData = ref<Record<string, unknown>>({})
const saving = ref(false)
const formError = ref<string | null>(null)
const loaded = ref(false)

onMounted(async () => {
  const id = entityId.value
  await fetchById(id)
  if (currentItem.value) {
    const data: Record<string, unknown> = {}
    for (const field of formFields.value) {
      data[field.key] =
        currentItem.value[field.key] ??
        (field.type === 'string-array' || field.type === 'relation-array' ? [] : '')
    }
    formData.value = data
  }
  loaded.value = true
})

async function handleSubmit() {
  if (!entityConfig.value || !currentItem.value) return
  saving.value = true
  formError.value = null
  try {
    const input = { ...formData.value }
    const id = getEntityId(currentItem.value)
    await updateEntity(id, input)
    router.push(`/admin/${entityPlural.value}/${entityId.value}`)
  } catch (e) {
    formError.value = String(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Header -->
    <div
      class="flex items-center gap-2 px-6 py-3 border-b border-(--ui-border) bg-(--ui-bg-elevated)"
    >
      <NuxtLink
        :to="`/admin/${entityPlural}`"
        class="text-sm text-(--ui-text-muted) hover:text-(--ui-text)"
      >
        {{ entityConfig?.pluralLabel }}
      </NuxtLink>
      <span class="text-(--ui-text-muted)">/</span>
      <NuxtLink
        :to="`/admin/${entityPlural}/${entityId}`"
        class="text-sm text-(--ui-text-muted) hover:text-(--ui-text)"
      >
        {{ entityId }}
      </NuxtLink>
      <span class="text-(--ui-text-muted)">/</span>
      <span class="text-sm font-semibold text-(--ui-text)">Edit</span>
    </div>

    <!-- Form -->
    <div class="flex-1 overflow-auto p-6">
      <div v-if="!loaded" class="text-(--ui-text-muted) text-sm">Loading...</div>

      <form v-else class="max-w-2xl space-y-4" @submit.prevent="handleSubmit">
        <div v-if="formError" class="text-red-600 text-sm p-3 rounded bg-red-50">
          {{ formError }}
        </div>

        <AdminFormField
          v-for="field in formFields"
          :key="field.key"
          :field="field"
          :model-value="formData[field.key]"
          @update:model-value="formData[field.key] = $event"
        />

        <div class="flex gap-3 pt-4">
          <button
            type="submit"
            :disabled="saving"
            class="px-4 py-2 text-sm rounded-md bg-(--ui-bg-inverted) text-(--ui-text-inverted) hover:opacity-90 transition-opacity disabled:opacity-50"
          >
            {{ saving ? 'Saving...' : 'Save Changes' }}
          </button>
          <NuxtLink
            :to="`/admin/${entityPlural}/${entityId}`"
            class="px-4 py-2 text-sm rounded-md border border-(--ui-border) text-(--ui-text) hover:bg-(--ui-bg-accented) transition-colors"
          >
            Cancel
          </NuxtLink>
        </div>
      </form>
    </div>
  </div>
</template>
