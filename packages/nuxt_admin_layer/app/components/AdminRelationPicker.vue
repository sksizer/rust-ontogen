<script setup lang="ts">
// A relation field picks a row of the target entity. Rows load once through
// the target's list method; the text box narrows them as you type. The raw id
// stays reachable behind "Edit id" for a row the list does not hold (a
// paginated target shows its first page only).
import { computed, onMounted, ref, watch } from 'vue'

type EntityRecord = Record<string, unknown>

const props = defineProps<{
  relationTo: string
  modelValue: unknown
}>()

const emit = defineEmits<{
  'update:modelValue': [value: unknown]
}>()

const transport = useTransport()
const { adminEntityMap } = useAdminRegistry()
const target = computed(() => adminEntityMap[props.relationTo])

const rows = ref<EntityRecord[]>([])
const loading = ref(false)
const loadError = ref<string | null>(null)
const query = ref('')
const open = ref(false)
const active = ref(0)
const editingId = ref(false)

const DISPLAY_KEYS = ['name', 'title', 'display_name'] as const

function idOf(row: EntityRecord): string {
  return String(row.id ?? row.contract_id ?? '')
}

function displayOf(row: EntityRecord): string {
  const key = DISPLAY_KEYS.find((k) => typeof row[k] === 'string' && (row[k] as string) !== '')
  return key ? (row[key] as string) : ''
}

function labelOf(row: EntityRecord): string {
  const display = displayOf(row)
  return display ? `${idOf(row)} — ${display}` : idOf(row)
}

const selected = computed(() => {
  const id = props.modelValue == null ? '' : String(props.modelValue)
  return id ? rows.value.find((row) => idOf(row) === id) : undefined
})

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return rows.value
  return rows.value.filter(
    (row) => idOf(row).toLowerCase().includes(q) || displayOf(row).toLowerCase().includes(q),
  )
})

watch(filtered, () => {
  active.value = 0
})

onMounted(load)

async function load() {
  const config = target.value
  if (!config) {
    loadError.value = `No admin entity '${props.relationTo}'`
    return
  }
  loading.value = true
  loadError.value = null
  try {
    const method = config.listMethod as keyof typeof transport
    const fn = transport[method] as unknown as (...args: unknown[]) => Promise<unknown>
    if (config.paginated) {
      const page = (await fn(...pageArgs(config, config.defaultLimit ?? 50, 0))) as { items: EntityRecord[] }
      rows.value = page.items
    } else {
      rows.value = (await fn()) as EntityRecord[]
    }
  } catch (e) {
    loadError.value = String(e)
  } finally {
    loading.value = false
  }
}

function pick(row: EntityRecord) {
  emit('update:modelValue', idOf(row))
  query.value = ''
  open.value = false
}

function clear() {
  emit('update:modelValue', undefined)
  query.value = ''
}

function onInput(event: Event) {
  query.value = (event.target as HTMLInputElement).value
  open.value = true
}

function onKeydown(event: KeyboardEvent) {
  if (!open.value && (event.key === 'ArrowDown' || event.key === 'Enter')) {
    open.value = true
    return
  }
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      active.value = Math.min(active.value + 1, filtered.value.length - 1)
      break
    case 'ArrowUp':
      event.preventDefault()
      active.value = Math.max(active.value - 1, 0)
      break
    case 'Enter': {
      event.preventDefault()
      const row = filtered.value[active.value]
      if (row) pick(row)
      break
    }
    case 'Escape':
      open.value = false
      break
  }
}

function closeSoon() {
  // Let a click on an option land before the list goes away.
  setTimeout(() => {
    open.value = false
  }, 150)
}
</script>

<template>
  <div data-admin-relation-picker>
    <div v-if="editingId" class="flex gap-2">
      <input
        type="text"
        :value="(modelValue as string) ?? ''"
        class="flex-1 px-3 py-2 text-sm rounded-md border border-(--ui-border) bg-(--ui-bg) text-(--ui-text) focus:outline-none focus:ring-1 focus:ring-(--ui-border-accented)"
        :placeholder="`${target?.label ?? relationTo} id`"
        @input="emit('update:modelValue', ($event.target as HTMLInputElement).value || undefined)"
      />
      <button
        type="button"
        class="px-3 py-1.5 text-sm rounded-md border border-(--ui-border) text-(--ui-text) hover:bg-(--ui-bg-accented)"
        @click="editingId = false"
      >
        Pick
      </button>
    </div>

    <div v-else class="relative">
      <div class="flex gap-2">
        <input
          type="text"
          :value="open ? query : selected ? labelOf(selected) : ((modelValue as string) ?? '')"
          role="combobox"
          :aria-expanded="open"
          class="flex-1 px-3 py-2 text-sm rounded-md border border-(--ui-border) bg-(--ui-bg) text-(--ui-text) focus:outline-none focus:ring-1 focus:ring-(--ui-border-accented)"
          :placeholder="loading ? 'Loading…' : `Search ${target?.pluralLabel?.toLowerCase() ?? relationTo}…`"
          @focus="open = true"
          @input="onInput"
          @keydown="onKeydown"
          @blur="closeSoon"
        />
        <button
          v-if="modelValue"
          type="button"
          class="px-3 py-1.5 text-sm rounded-md border border-(--ui-border) text-(--ui-text-muted) hover:bg-(--ui-bg-accented)"
          title="Clear"
          @click="clear"
        >
          &times;
        </button>
        <button
          type="button"
          class="px-3 py-1.5 text-sm rounded-md border border-(--ui-border) text-(--ui-text-muted) hover:bg-(--ui-bg-accented)"
          @click="editingId = true"
        >
          Edit id
        </button>
      </div>

      <div v-if="loadError" class="mt-1 text-xs text-red-600">{{ loadError }}</div>

      <ul
        v-if="open && !loading"
        role="listbox"
        class="absolute z-10 mt-1 max-h-64 w-full overflow-auto rounded-md border border-(--ui-border) bg-(--ui-bg-elevated) shadow-md text-sm"
      >
        <li v-if="filtered.length === 0" class="px-3 py-2 text-(--ui-text-muted)">No match</li>
        <li
          v-for="(row, idx) in filtered"
          :key="idOf(row)"
          role="option"
          :aria-selected="idOf(row) === String(modelValue ?? '')"
          class="px-3 py-1.5 cursor-pointer text-(--ui-text)"
          :class="idx === active ? 'bg-(--ui-bg-accented)' : 'hover:bg-(--ui-bg-accented)'"
          @mousedown.prevent="pick(row)"
          @mousemove="active = idx"
        >
          {{ labelOf(row) }}
        </li>
      </ul>
    </div>
  </div>
</template>
