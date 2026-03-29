import {
  adminEntities,
  adminEntityMap,
  adminEntityByPlural,
  adminFieldDefs,
  type AdminEntityConfig,
  type AdminFieldDef,
} from '~/admin/generated/admin-registry'

/**
 * Provides admin entity registry and field definitions for iron-log.
 * Overrides the default empty useAdminRegistry from @ontogen/admin-layer.
 */
export function useAdminRegistry(): {
  adminEntities: AdminEntityConfig[]
  adminEntityMap: Record<string, AdminEntityConfig>
  adminEntityByPlural: Record<string, AdminEntityConfig>
  adminFieldDefs: Record<string, AdminFieldDef[]>
} {
  return {
    adminEntities,
    adminEntityMap,
    adminEntityByPlural,
    adminFieldDefs,
  }
}
