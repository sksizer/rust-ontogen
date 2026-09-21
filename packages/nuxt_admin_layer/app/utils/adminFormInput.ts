import type { AdminFieldDef } from '@ontogen/admin-types'

type EntityRecord = Record<string, unknown>

/**
 * Form value for a field the record has no value for. Arrays start empty,
 * numbers start null (the empty string is not a number and the server
 * rejects it), and everything else starts as the empty string.
 *
 * Booleans: a *required* boolean (the registry's `is_required` matches plain
 * `bool`, never `Option<bool>` — see src/clients/generators/admin.rs) always
 * has a real value at the database, so `false` is a safe default. A
 * *non-required* boolean is an `Option<bool>` and can genuinely be `null`
 * ("not yet decided"), which is a different state from an explicit `false`;
 * seeding it `false` would make an untouched checkbox indistinguishable from
 * one the user deliberately unchecked, and toUpdateInput would then persist
 * that fabricated `false` over a stored `null` on every untouched save. A
 * non-required boolean therefore starts `null`, rendered unchecked by
 * AdminFormField's `(modelValue as boolean) ?? false`; only a user click
 * (which always emits a real `true`/`false`) moves it off `null`.
 */
export function emptyFormValue(field: AdminFieldDef): unknown {
  if (field.type === 'string-array' || field.type === 'relation-array') return []
  if (field.type === 'number') return null
  if (field.type === 'boolean') return field.required ? false : null
  return ''
}

/** Form state for the edit page: the record's current values, blanks filled per field type. */
export function initEditFormData(fields: AdminFieldDef[], item: EntityRecord): EntityRecord {
  const data: EntityRecord = {}
  for (const field of fields) {
    data[field.key] = item[field.key] ?? emptyFormValue(field)
  }
  return data
}

function isBlank(value: unknown): boolean {
  return value === '' || value === undefined || value === null
}

/**
 * Wire payload for the create call, derived from the new-record form's state.
 *
 * A blank optional string, enum or relation is left out (undefined is dropped
 * from the payload), so the server applies its own default rather than
 * receiving '' — which no enum or relation deserializes. Other field types
 * pass through as the form holds them.
 */
export function toCreateInput(fields: AdminFieldDef[], formData: EntityRecord): EntityRecord {
  const input: EntityRecord = { ...formData }
  for (const field of fields) {
    if (field.required || !isBlank(input[field.key])) continue
    if (field.type === 'string' || field.type === 'enum' || field.type === 'relation') {
      input[field.key] = undefined
    }
  }
  return input
}

/**
 * Wire payload for the update call, derived from the edit form's state.
 *
 * The form shows the record's current values, so an emptied optional field
 * means "none". AdminFormField reports an emptied input as '' (text) or
 * undefined (number, enum, relation), and an untouched blank is whatever
 * `emptyFormValue` seeded. All three become null, the one spelling that both
 * deserializes and is present on the wire: '' fails to parse as a number or
 * enum, and undefined is dropped from the JSON payload, which the server
 * reads as "leave the stored value alone".
 */
export function toUpdateInput(fields: AdminFieldDef[], formData: EntityRecord): EntityRecord {
  const input: EntityRecord = { ...formData }
  for (const field of fields) {
    if (!field.required && isBlank(input[field.key])) input[field.key] = null
  }
  return input
}
