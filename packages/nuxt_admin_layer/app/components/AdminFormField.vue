<script setup lang="ts">
import type { AdminFieldDef } from '../../admin-fields'

const props = defineProps<{
  field: AdminFieldDef
  modelValue: unknown
}>()

const emit = defineEmits<{
  'update:modelValue': [value: unknown]
}>()

const arrayInput = ref('')

function addArrayItem() {
  const trimmed = arrayInput.value.trim()
  if (!trimmed) return
  const current = (props.modelValue as string[]) ?? []
  emit('update:modelValue', [...current, trimmed])
  arrayInput.value = ''
}

function removeArrayItem(index: number) {
  const current = (props.modelValue as string[]) ?? []
  emit(
    'update:modelValue',
    current.filter((_, i) => i !== index),
  )
}
</script>

<template>
  <div>
    <label class="block text-sm font-medium text-(--ui-text-muted) mb-1">
      {{ field.label }}
      <span v-if="field.required" class="text-red-500">*</span>
    </label>

    <!-- Text area for body/text fields -->
    <textarea
      v-if="field.type === 'text'"
      :value="(modelValue as string) ?? ''"
      rows="6"
      class="w-full px-3 py-2 text-sm rounded-md border border-(--ui-border) bg-(--ui-bg) text-(--ui-text) focus:outline-none focus:ring-1 focus:ring-(--ui-border-accented) resize-y"
      @input="emit('update:modelValue', ($event.target as HTMLTextAreaElement).value)"
    />

    <!-- Enum select -->
    <select
      v-else-if="field.type === 'enum'"
      :value="(modelValue as string) ?? ''"
      class="w-full px-3 py-2 text-sm rounded-md border border-(--ui-border) bg-(--ui-bg) text-(--ui-text) focus:outline-none focus:ring-1 focus:ring-(--ui-border-accented)"
      @change="emit('update:modelValue', ($event.target as HTMLSelectElement).value || undefined)"
    >
      <option value="">— Select —</option>
      <option v-for="val in field.enumValues" :key="val" :value="val">
        {{ val }}
      </option>
    </select>

    <!-- String array / Relation array -->
    <div v-else-if="field.type === 'string-array' || field.type === 'relation-array'">
      <div class="flex flex-wrap gap-1.5 mb-2">
        <span
          v-for="(item, idx) in (modelValue as string[]) ?? []"
          :key="idx"
          class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs bg-(--ui-bg-accented) text-(--ui-text)"
        >
          {{ item }}
          <button
            type="button"
            class="text-(--ui-text-muted) hover:text-red-500"
            @click="removeArrayItem(idx)"
          >
            &times;
          </button>
        </span>
      </div>
      <div class="flex gap-2">
        <input
          v-model="arrayInput"
          type="text"
          class="flex-1 px-3 py-1.5 text-sm rounded-md border border-(--ui-border) bg-(--ui-bg) text-(--ui-text) focus:outline-none focus:ring-1 focus:ring-(--ui-border-accented)"
          :placeholder="`Add ${field.label.toLowerCase()}...`"
          @keydown.enter.prevent="addArrayItem"
        />
        <button
          type="button"
          class="px-3 py-1.5 text-sm rounded-md border border-(--ui-border) text-(--ui-text) hover:bg-(--ui-bg-accented)"
          @click="addArrayItem"
        >
          Add
        </button>
      </div>
    </div>

    <!-- Single relation (rendered as text input for now) -->
    <input
      v-else-if="field.type === 'relation'"
      type="text"
      :value="(modelValue as string) ?? ''"
      class="w-full px-3 py-2 text-sm rounded-md border border-(--ui-border) bg-(--ui-bg) text-(--ui-text) focus:outline-none focus:ring-1 focus:ring-(--ui-border-accented)"
      :placeholder="`${field.relationTo} ID`"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value || undefined)"
    />

    <!-- Number input -->
    <input
      v-else-if="field.type === 'number'"
      type="number"
      :value="(modelValue as number) ?? ''"
      class="w-full px-3 py-2 text-sm rounded-md border border-(--ui-border) bg-(--ui-bg) text-(--ui-text) focus:outline-none focus:ring-1 focus:ring-(--ui-border-accented)"
      @input="emit('update:modelValue', Number(($event.target as HTMLInputElement).value))"
    />

    <!-- Default string input -->
    <input
      v-else
      type="text"
      :value="(modelValue as string) ?? ''"
      class="w-full px-3 py-2 text-sm rounded-md border border-(--ui-border) bg-(--ui-bg) text-(--ui-text) focus:outline-none focus:ring-1 focus:ring-(--ui-border-accented)"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    />
  </div>
</template>
