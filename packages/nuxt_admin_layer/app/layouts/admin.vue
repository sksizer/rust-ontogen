<script setup lang="ts">
// Auto-imported from consuming app via useAdminRegistry()
const { adminEntities } = useAdminRegistry()

const sidebarOpen = ref(true)

function toggleSidebar() {
  sidebarOpen.value = !sidebarOpen.value
}

const entityNavItems = computed(() =>
  adminEntities.map((e) => ({
    label: e.pluralLabel,
    to: `/admin/${e.plural}`,
    key: e.key,
  })),
)
</script>

<template>
  <div class="flex h-screen w-screen bg-(--ui-bg)">
    <!-- Sidebar -->
    <aside
      class="flex flex-col border-r border-(--ui-border) bg-(--ui-bg-elevated) transition-all duration-200 ease-in-out"
      :class="sidebarOpen ? 'w-52' : 'w-12'"
    >
      <div
        class="flex items-center px-2 py-2"
        :class="sidebarOpen ? 'justify-between' : 'justify-center'"
      >
        <NuxtLink
          v-if="sidebarOpen"
          to="/admin"
          class="text-sm font-semibold text-(--ui-text) px-2 truncate"
        >
          Admin
        </NuxtLink>
        <button
          class="flex items-center justify-center w-8 h-8 rounded-md text-(--ui-text-muted) hover:text-(--ui-text) hover:bg-(--ui-bg-accented) transition-colors"
          @click="toggleSidebar"
        >
          <UIcon
            :name="sidebarOpen ? 'i-heroicons-chevron-left' : 'i-heroicons-chevron-right'"
            class="w-4 h-4"
          />
        </button>
      </div>

      <nav class="flex-1 flex flex-col gap-0.5 px-1.5 overflow-y-auto">
        <NuxtLink
          v-for="item in entityNavItems"
          :key="item.key"
          :to="item.to"
          class="group flex items-center gap-2.5 rounded-md px-2 py-1.5 text-sm transition-colors text-(--ui-text-muted) hover:text-(--ui-text) hover:bg-(--ui-bg-accented)"
          active-class="!text-(--ui-text) bg-(--ui-bg-accented) font-medium"
          :title="!sidebarOpen ? item.label : undefined"
        >
          <span v-if="sidebarOpen" class="truncate">{{ item.label }}</span>
          <span v-else class="text-xs font-mono">{{ item.label.slice(0, 2) }}</span>
        </NuxtLink>
      </nav>

      <!-- Back to app -->
      <div class="border-t border-(--ui-border) px-1.5 py-1.5">
        <NuxtLink
          to="/"
          class="group flex items-center gap-2.5 rounded-md px-2 py-1.5 text-sm transition-colors text-(--ui-text-muted) hover:text-(--ui-text) hover:bg-(--ui-bg-accented)"
        >
          <UIcon name="i-heroicons-arrow-left" class="w-4 h-4 shrink-0" />
          <span v-if="sidebarOpen" class="truncate">Back to App</span>
        </NuxtLink>
      </div>
    </aside>

    <!-- Main content -->
    <main class="flex-1 overflow-hidden">
      <slot />
    </main>
  </div>
</template>
