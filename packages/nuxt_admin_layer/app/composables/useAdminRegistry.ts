import type { AdminEntityConfig } from './useAdminEntity'
import { adminFieldDefs as layerFieldDefs, type AdminFieldDef } from '../../admin-fields'

/**
 * Provides admin entity registry and field definitions.
 *
 * This composable is designed to be overridden by the consuming app.
 * The consuming app should provide its own useAdminRegistry() that returns
 * entity configs from the generated admin-registry.ts and field definitions
 * from admin-fields.ts (generated or hand-maintained).
 *
 * This base implementation returns empty defaults. The consuming app MUST
 * override this by providing its own useAdminRegistry composable that
 * populates the data from generated files.
 */
export function useAdminRegistry(): {
  adminEntities: AdminEntityConfig[]
  adminEntityMap: Record<string, AdminEntityConfig>
  adminEntityByPlural: Record<string, AdminEntityConfig>
  adminFieldDefs: Record<string, AdminFieldDef[]>
} {
  // This will be overridden by the consuming app's auto-imported version.
  // If not overridden, returns empty defaults (no entities shown).
  return {
    adminEntities: [],
    adminEntityMap: {},
    adminEntityByPlural: {},
    adminFieldDefs: layerFieldDefs,
  }
}
