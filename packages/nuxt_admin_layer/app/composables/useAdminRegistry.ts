import type { AdminEntityConfig, AdminFieldDef } from '@ontogen/admin-types'

// #admin-registry is a virtual alias set by the layer's Nuxt module.
// It resolves to the consuming app's app/admin/generated/admin-registry.ts at build time.
// @ts-ignore — alias is only resolvable during Nuxt build, not standalone tsc
import {
  adminEntities,
  adminEntityMap,
  adminEntityByPlural,
  adminFieldDefs,
} from '#admin-registry'

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
