/**
 * Formats an entity field value for display.
 * Returns null for empty/missing values, joins arrays with commas,
 * and converts everything else to a string.
 */
export function formatFieldValue(item: Record<string, unknown>, key: string): string | null {
  const val = item[key]
  if (val === null || val === undefined) return null
  if (Array.isArray(val)) return val.length > 0 ? val.join(', ') : null
  return String(val)
}
