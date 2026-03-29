<script setup lang="ts">
definePageMeta({ layout: 'admin' })

const route = useRoute()
const router = useRouter()
const entityPlural = computed(() => route.params.entity as string)

const { adminEntityByPlural } = useAdminRegistry()
const entityConfig = computed(() => adminEntityByPlural[entityPlural.value])

const { fields, createEntity } = useAdminEntity(entityPlural.value)

const formFields = computed(() => fields.value.filter((f) => f.showInForm && !f.readOnly))

const formData = ref<Record<string, unknown>>({})
const saving = ref(false)
const formError = ref<string | null>(null)

// Initialize defaults
onMounted(() => {
  const data: Record<string, unknown> = {}
  for (const field of formFields.value) {
    if (field.type === 'string-array' || field.type === 'relation-array') {
      data[field.key] = []
    } else {
      data[field.key] = field.type === 'number' ? null : ''
    }
  }
  formData.value = data
})

async function handleSubmit() {
  if (!entityConfig.value) return
  saving.value = true
  formError.value = null
  try {
    const input = { ...formData.value }
    // Clean up empty optional fields
    for (const field of formFields.value) {
      if (!field.required && (input[field.key] === '' || input[field.key] === null)) {
        if (field.type === 'string' || field.type === 'enum' || field.type === 'relation') {
          input[field.key] = undefined
        }
      }
    }
    await createEntity(input)
    router.push(`/admin/${entityPlural.value}`)
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
      <span class="text-sm font-semibold text-(--ui-text)">New</span>
    </div>

    <!-- Form -->
    <div class="flex-1 overflow-auto p-6">
      <form class="max-w-2xl space-y-4" @submit.prevent="handleSubmit">
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
            {{ saving ? 'Creating...' : `Create ${entityConfig?.label}` }}
          </button>
          <NuxtLink
            :to="`/admin/${entityPlural}`"
            class="px-4 py-2 text-sm rounded-md border border-(--ui-border) text-(--ui-text) hover:bg-(--ui-bg-accented) transition-colors"
          >
            Cancel
          </NuxtLink>
        </div>
      </form>
    </div>
  </div>
</template>
