// AdminFormField's per-FieldType branch selection, mounted for real: the
// defect (a missing 'boolean' branch) lives in the template's v-if chain,
// so this is exercised through the component rather than a pure function.
import type { AdminFieldDef } from '@ontogen/admin-types'
import { mount } from '@vue/test-utils'
import { describe, expect, test } from 'vitest'

import AdminFormField from '../app/components/AdminFormField.vue'

const booleanField: AdminFieldDef = { key: 'active', label: 'Active', type: 'boolean', required: true }

describe('AdminFormField boolean fields', () => {
  test('renders a checkbox instead of falling through to the text input', () => {
    const wrapper = mount(AdminFormField, { props: { field: booleanField, modelValue: true } })

    const checkbox = wrapper.find('input[type="checkbox"]')
    expect(checkbox.exists()).toBe(true)
    expect((checkbox.element as HTMLInputElement).checked).toBe(true)
    expect(wrapper.find('input[type="text"]').exists()).toBe(false)
  })

  test('an unset boolean value renders unchecked rather than as an empty string', () => {
    const wrapper = mount(AdminFormField, { props: { field: booleanField, modelValue: undefined } })

    const checkbox = wrapper.find('input[type="checkbox"]')
    expect(checkbox.exists()).toBe(true)
    expect((checkbox.element as HTMLInputElement).checked).toBe(false)
  })

  test('toggling the checkbox emits a real boolean, never the string "true"/"false"', async () => {
    const wrapper = mount(AdminFormField, { props: { field: booleanField, modelValue: false } })

    await wrapper.find('input[type="checkbox"]').setValue(true)

    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual([true])
  })
})
