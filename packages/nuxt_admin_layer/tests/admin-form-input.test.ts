// The form-state rules the create and edit pages delegate to, pinned on
// their own; admin-edit-page.test.ts covers the edit page's wiring to them.
import type { AdminFieldDef } from '@ontogen/admin-types'
import { describe, expect, test } from 'vitest'

import { coerceNumberInput } from '../app/components/admin-form-field-number'
import { emptyFormValue, initEditFormData, toCreateInput, toUpdateInput } from '../app/utils/adminFormInput'

const fields: AdminFieldDef[] = [
  { key: 'duration_secs', label: 'Duration', type: 'number', required: true },
  { key: 'tags', label: 'Tags', type: 'string-array' },
  { key: 'rating', label: 'Rating', type: 'number' },
  { key: 'notes', label: 'Notes', type: 'string' },
  { key: 'completion_kind', label: 'Completion', type: 'enum', enumValues: ['finished'] },
  { key: 'profile', label: 'Profile', type: 'relation', relationTo: 'profile' },
  { key: 'active', label: 'Active', type: 'boolean' },
]

describe('emptyFormValue', () => {
  test('arrays start empty, numbers start null, everything else starts blank', () => {
    expect(emptyFormValue({ key: 'tags', label: '', type: 'string-array' })).toEqual([])
    expect(emptyFormValue({ key: 'refs', label: '', type: 'relation-array' })).toEqual([])
    expect(emptyFormValue({ key: 'rating', label: '', type: 'number' })).toBeNull()
    expect(emptyFormValue({ key: 'notes', label: '', type: 'string' })).toBe('')
  })

  test('a non-required boolean (Option<bool>) starts null, not false', () => {
    // required is undefined here, matching a real Option<bool> field def.
    expect(emptyFormValue({ key: 'active', label: '', type: 'boolean' })).toBeNull()
  })

  test('a required boolean (plain bool) starts false — it always has a real stored value', () => {
    expect(emptyFormValue({ key: 'active', label: '', type: 'boolean', required: true })).toBe(false)
  })
})

describe('initEditFormData', () => {
  test('a stored null number becomes null, not the empty string', () => {
    const data = initEditFormData(fields, { duration_secs: 60, rating: null })
    expect(data.rating).toBeNull()
    expect(data.duration_secs).toBe(60)
  })

  test('a stored null array field becomes an empty array', () => {
    expect(initEditFormData(fields, { tags: null }).tags).toEqual([])
  })

  test('a stored null non-required boolean stays null, not false', () => {
    // `fields`' `active` field has no `required`, i.e. an Option<bool>.
    expect(initEditFormData(fields, { active: null }).active).toBeNull()
    expect(initEditFormData(fields, {}).active).toBeNull() // key absent entirely, same as stored null
  })

  test('a stored null required boolean still falls back to false', () => {
    const requiredActive: AdminFieldDef[] = [{ key: 'active', label: 'Active', type: 'boolean', required: true }]
    expect(initEditFormData(requiredActive, { active: null }).active).toBe(false)
  })
})

describe('toUpdateInput', () => {
  test('untouched null optional fields are sent as null, never the empty string', () => {
    const data = initEditFormData(fields, {
      duration_secs: 60,
      rating: null,
      notes: null,
      completion_kind: null,
      profile: null,
    })
    const input = toUpdateInput(fields, data)
    expect(input).toEqual({
      duration_secs: 60,
      tags: [],
      rating: null,
      notes: null,
      completion_kind: null,
      profile: null,
      // Untouched non-required boolean: null in, null out. Sending `false`
      // here would persist a fabricated "unchecked" over a stored `null`.
      active: null,
    })
  })

  test('a cleared number field (coerceNumberInput) reaches the wire as an explicit null', () => {
    const data = initEditFormData(fields, { duration_secs: 60, rating: 4, notes: 'x' })
    data.rating = coerceNumberInput('')
    const input = toUpdateInput(fields, data)
    expect(input.rating).toBeNull()
    expect(JSON.parse(JSON.stringify(input)).rating).toBeNull()
  })

  test('a cleared enum, relation or string field reaches the wire as null', () => {
    const data = initEditFormData(fields, {
      duration_secs: 60,
      notes: 'x',
      completion_kind: 'finished',
      profile: 'p1',
    })
    data.completion_kind = undefined
    data.profile = undefined
    data.notes = ''
    const input = toUpdateInput(fields, data)
    expect(input.completion_kind).toBeNull()
    expect(input.profile).toBeNull()
    expect(input.notes).toBeNull()
  })

  test('required fields, real values and a true boolean pass through untouched', () => {
    const data = initEditFormData(fields, {
      duration_secs: 60,
      rating: 0,
      notes: 'x',
      tags: ['a'],
      active: true,
    })
    const input = toUpdateInput(fields, data)
    expect(input.duration_secs).toBe(60)
    expect(input.rating).toBe(0)
    expect(input.notes).toBe('x')
    expect(input.tags).toEqual(['a'])
    expect(input.active).toBe(true)
  })

  test('does not mutate the form state it reads', () => {
    const data = initEditFormData(fields, { duration_secs: 60, rating: 4 })
    data.rating = undefined
    toUpdateInput(fields, data)
    expect(data.rating).toBeUndefined()
  })
})

describe('toCreateInput', () => {
  test('blank optional string, enum and relation fields are left out of the payload', () => {
    const input = toCreateInput(fields, {
      duration_secs: 60,
      tags: [],
      rating: null,
      notes: '',
      completion_kind: '',
      profile: null,
      active: false,
    })
    expect(JSON.parse(JSON.stringify(input))).toEqual({
      duration_secs: 60,
      tags: [],
      rating: null,
      active: false,
    })
  })

  test('required fields and real values pass through untouched', () => {
    const input = toCreateInput(fields, {
      duration_secs: 60,
      notes: 'x',
      completion_kind: 'finished',
    })
    expect(input).toEqual({ duration_secs: 60, notes: 'x', completion_kind: 'finished' })
  })

  test('an untouched non-required boolean reaches create as an explicit null', () => {
    // toCreateInput only maps string/enum/relation blanks to undefined;
    // booleans pass through whatever emptyFormValue seeded. Unlike update,
    // there is no existing stored value to preserve, so null is unambiguous.
    const activeField = fields.find((f) => f.key === 'active')
    if (!activeField) throw new Error('fixture missing the active field')
    const formData = { duration_secs: 60, active: emptyFormValue(activeField) }
    const input = toCreateInput(fields, formData)
    expect(input.active).toBeNull()
  })
})
