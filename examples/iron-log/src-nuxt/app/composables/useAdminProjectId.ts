import { ref } from 'vue'

/**
 * Iron-log is a single-project app — no multi-tenancy.
 * Returns undefined to skip project-scoping in the admin layer.
 */
export function useAdminProjectId() {
  return ref<string | undefined>(undefined)
}
