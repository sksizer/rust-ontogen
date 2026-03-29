import type { AdminFieldDef } from '../../admin-fields'

/**
 * Deep-merge field overrides into generated field definitions.
 *
 * For each entity key in `overrides`, finds matching fields by `key`
 * and merges the override properties on top of the generated base.
 * Unmatched overrides are appended (allowing hand-added fields).
 * Entities without overrides pass through unchanged.
 */
export function mergeAdminFields(
  generated: Record<string, AdminFieldDef[]>,
  overrides: Record<string, Partial<AdminFieldDef>[]>,
): Record<string, AdminFieldDef[]> {
  const result: Record<string, AdminFieldDef[]> = { ...generated }

  for (const [entityKey, fieldOverrides] of Object.entries(overrides)) {
    const baseFields = result[entityKey]
    if (!baseFields) {
      // Override for an entity not in generated — skip (or could add)
      continue
    }

    const merged = baseFields.map((field) => {
      const override = fieldOverrides.find((o) => o.key === field.key)
      if (override) {
        return { ...field, ...override } as AdminFieldDef
      }
      return field
    })

    // Append any overrides for fields that don't exist in generated
    for (const override of fieldOverrides) {
      if (override.key && !baseFields.some((f) => f.key === override.key)) {
        merged.push(override as AdminFieldDef)
      }
    }

    result[entityKey] = merged
  }

  return result
}
