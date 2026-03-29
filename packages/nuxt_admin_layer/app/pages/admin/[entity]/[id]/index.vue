<script setup lang="ts">
import DOMPurify from 'dompurify'
import { marked } from 'marked'

definePageMeta({ layout: 'admin' })

const route = useRoute()
const router = useRouter()
const entityPlural = computed(() => route.params.entity as string)
const entityId = computed(() => route.params.id as string)

const { adminEntityByPlural, adminEntityMap } = useAdminRegistry()
const entityConfig = computed(() => adminEntityByPlural[entityPlural.value])

const { currentItem, loading, error, fields, fetchById, deleteEntity, getEntityId } =
  useAdminEntity(entityPlural.value)

const deleting = ref(false)

const detailFields = computed(() => fields.value.filter((f) => f.showInDetail && !f.isBody))
const bodyField = computed(() => fields.value.find((f) => f.isBody))
const parsedBody = ref('')

watch(
  () => currentItem.value,
  async (item) => {
    if (item && bodyField.value) {
      const body = item[bodyField.value.key]
      parsedBody.value = body ? DOMPurify.sanitize(await marked.parse(String(body))) : ''
    } else {
      parsedBody.value = ''
    }
  },
  { immediate: true },
)

onMounted(() => {
  const id = entityId.value
  fetchById(id)
})

async function handleDelete() {
  if (!currentItem.value || !entityConfig.value) return
  deleting.value = true
  try {
    const id = getEntityId(currentItem.value)
    await deleteEntity(id)
    router.push(`/admin/${entityPlural.value}`)
  } catch (e) {
    alert(`Delete failed: ${e}`)
  } finally {
    deleting.value = false
  }
}

function navigateToEdit() {
  router.push(`/admin/${entityPlural.value}/${entityId.value}/edit`)
}

function resolveRelationRoute(field: { relationTo?: string }, value: unknown): string | null {
  if (!field.relationTo || !value) return null
  const targetConfig = adminEntityMap[field.relationTo]
  if (!targetConfig) return null
  return `/admin/${targetConfig.plural}/${encodeURIComponent(String(value))}`
}
</script>

<template>
  <div class="flex flex-col h-full">
    <!-- Header -->
    <div
      class="flex items-center justify-between px-6 py-3 border-b border-(--ui-border) bg-(--ui-bg-elevated)"
    >
      <div class="flex items-center gap-2">
        <NuxtLink
          :to="`/admin/${entityPlural}`"
          class="text-sm text-(--ui-text-muted) hover:text-(--ui-text)"
        >
          {{ entityConfig?.pluralLabel }}
        </NuxtLink>
        <span class="text-(--ui-text-muted)">/</span>
        <span class="text-sm font-semibold text-(--ui-text)">{{ entityId }}</span>
      </div>
      <div class="flex gap-2">
        <button
          class="px-3 py-1.5 text-sm rounded-md border border-(--ui-border) text-(--ui-text) hover:bg-(--ui-bg-accented) transition-colors"
          @click="navigateToEdit"
        >
          Edit
        </button>
        <button
          class="px-3 py-1.5 text-sm rounded-md bg-red-600 text-white hover:bg-red-700 transition-colors"
          :disabled="deleting"
          @click="handleDelete"
        >
          {{ deleting ? 'Deleting...' : 'Delete' }}
        </button>
      </div>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-auto p-6">
      <div v-if="loading" class="text-(--ui-text-muted) text-sm">Loading...</div>
      <div v-else-if="error" class="text-red-600 text-sm">{{ error }}</div>

      <template v-else-if="currentItem">
        <!-- Detail fields -->
        <table class="w-full text-sm mb-6">
          <tbody>
            <tr
              v-for="field in detailFields"
              :key="field.key"
              class="border-b border-(--ui-border)"
            >
              <td
                class="py-2 pr-4 text-(--ui-text-muted) font-medium whitespace-nowrap w-40 align-top"
              >
                {{ field.label }}
              </td>
              <td class="py-2 text-(--ui-text)">
                <!-- Relation link -->
                <template
                  v-if="
                    field.type === 'relation' && resolveRelationRoute(field, currentItem[field.key])
                  "
                >
                  <NuxtLink
                    :to="resolveRelationRoute(field, currentItem[field.key])!"
                    class="text-blue-600 hover:underline"
                  >
                    {{ currentItem[field.key] }}
                  </NuxtLink>
                </template>

                <!-- Relation array -->
                <template
                  v-else-if="
                    field.type === 'relation-array' &&
                    Array.isArray(currentItem[field.key]) &&
                    (currentItem[field.key] as unknown[]).length > 0
                  "
                >
                  <div class="flex flex-wrap gap-1.5">
                    <NuxtLink
                      v-for="relId in currentItem[field.key] as string[]"
                      :key="relId"
                      :to="resolveRelationRoute(field, relId) ?? '#'"
                      class="inline-block px-2 py-0.5 rounded text-xs bg-(--ui-bg-accented) text-blue-600 hover:underline"
                    >
                      {{ relId }}
                    </NuxtLink>
                  </div>
                </template>

                <!-- String array -->
                <template
                  v-else-if="
                    field.type === 'string-array' &&
                    Array.isArray(currentItem[field.key]) &&
                    (currentItem[field.key] as unknown[]).length > 0
                  "
                >
                  <div class="flex flex-wrap gap-1.5">
                    <span
                      v-for="(val, idx) in currentItem[field.key] as string[]"
                      :key="idx"
                      class="inline-block px-2 py-0.5 rounded text-xs bg-(--ui-bg-accented) text-(--ui-text-muted)"
                    >
                      {{ val }}
                    </span>
                  </div>
                </template>

                <!-- Default -->
                <template v-else>
                  {{ formatFieldValue(currentItem, field.key) ?? '—' }}
                </template>
              </td>
            </tr>
          </tbody>
        </table>

        <!-- Body (markdown) -->
        <div v-if="parsedBody" class="mt-4">
          <h3 class="text-sm font-medium text-(--ui-text-muted) mb-2">
            {{ bodyField?.label ?? 'Body' }}
          </h3>
          <!-- eslint-disable-next-line vue/no-v-html -->
          <div class="prose prose-sm max-w-none dark:prose-invert" v-html="parsedBody" />
        </div>

        <!-- Inline relation tables -->
        <AdminRelationTables
          v-if="currentItem"
          :entity-key="entityConfig?.key ?? ''"
          :entity-id="String(getEntityId(currentItem))"
          :fields="fields"
        />
      </template>

      <div v-else class="text-(--ui-text-muted) text-sm">Not found.</div>
    </div>
  </div>
</template>
