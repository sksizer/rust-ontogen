// The coercion rule AdminFormField.vue's number-input handler delegates to,
// pinned on its own; admin-form-field.test.ts and admin-edit-page.test.ts
// cover the handler wiring.
import { describe, expect, test } from 'vitest'

import { coerceNumberInput } from '../app/components/admin-form-field-number'

describe('coerceNumberInput', () => {
  test('a cleared input (the empty string) coerces to undefined, not 0', () => {
    expect(coerceNumberInput('')).toBeUndefined()
  })

  test('a real numeric string coerces to a number', () => {
    expect(coerceNumberInput('42')).toBe(42)
    expect(coerceNumberInput('0')).toBe(0)
    expect(coerceNumberInput('-3.5')).toBe(-3.5)
  })
})
