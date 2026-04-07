<script setup lang="ts">
/**
 * Reusable pagination controls for admin entity list pages.
 *
 * Displays previous/next buttons, current page info, and total item count.
 * Emits navigation events for the parent to handle.
 */
defineProps<{
  page: number
  totalPages: number
  total: number
  limit: number
}>()

const emit = defineEmits<{
  prev: []
  next: []
  goTo: [page: number]
}>()
</script>

<template>
  <div class="admin-pagination">
    <button :disabled="page <= 1" @click="emit('prev')">
      Previous
    </button>
    <span class="admin-pagination-info">
      Page {{ page }} of {{ totalPages }}
      <span class="admin-pagination-total">({{ total }} items)</span>
    </span>
    <button :disabled="page >= totalPages" @click="emit('next')">
      Next
    </button>
  </div>
</template>

<style scoped>
.admin-pagination {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.75rem 0;
}
.admin-pagination button {
  padding: 0.375rem 0.75rem;
  border: 1px solid #d1d5db;
  border-radius: 0.375rem;
  background: white;
  cursor: pointer;
}
.admin-pagination button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.admin-pagination-info {
  font-size: 0.875rem;
  color: #6b7280;
}
.admin-pagination-total {
  color: #9ca3af;
}
</style>
