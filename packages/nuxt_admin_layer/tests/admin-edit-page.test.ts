/**
 * The generic edit page mounted for real, over a small entity registry and a
 * scripted transport, the way `useAdminRegistry`/`useTransport` would be
 * auto-imported inside a consuming Nuxt app. `useAdminEntity` is imported for
 * real too, so these tests exercise the actual fetchById-error path, not a
 * stand-in for it. `NuxtLink` is stubbed (a Nuxt built-in, unavailable
 * standalone); `AdminFormField` is the real component.
 */
import type { AdminEntityConfig } from '@ontogen/admin-types'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, test } from 'vitest'
import { defineComponent, h } from 'vue'

import AdminFormField from '../app/components/AdminFormField.vue'
import { useAdminEntity } from '../app/composables/useAdminEntity'
import { initEditFormData, toUpdateInput } from '../app/utils/adminFormInput'
import EditPage from '../app/pages/admin/[entity]/[id]/edit.vue'

type Rec = Record<string, unknown>
type Transport = Record<string, (...args: unknown[]) => Promise<unknown>>

const widget: AdminEntityConfig = {
  key: 'widget',
  plural: 'widgets',
  label: 'Widget',
  pluralLabel: 'Widgets',
  idType: 'string',
  listMethod: 'widgetList',
  getMethod: 'widgetGetById',
  createMethod: 'widgetCreate',
  updateMethod: 'widgetUpdate',
  deleteMethod: 'widgetDelete',
  returnType: 'Widget',
  createInputType: 'CreateWidgetInput',
  updateInputType: 'UpdateWidgetInput',
  fields: [
    { key: 'id', label: 'Id', type: 'string', required: true, showInForm: true, isId: true },
    { key: 'label', label: 'Label', type: 'string', required: true, showInForm: true },
    { key: 'rating', label: 'Rating', type: 'number', showInForm: true },
    { key: 'notes', label: 'Notes', type: 'text', showInForm: true },
    {
      key: 'completion_kind',
      label: 'Completion Kind',
      type: 'enum',
      enumValues: ['finished', 'abandoned'],
      showInForm: true,
    },
    // Non-required (no `required: true`) — an Option<bool>, which can be a
    // genuine null distinct from an explicit false.
    { key: 'active', label: 'Active', type: 'boolean', showInForm: true },
  ],
}

const NuxtLinkStub = defineComponent({
  name: 'NuxtLink',
  props: { to: { type: String, default: '' } },
  setup(props, { slots }) {
    return () => h('a', { href: props.to }, slots.default?.())
  },
})

let transport: Transport = {}
let routeId = ''
const pushes: string[] = []

function stubApp() {
  Object.assign(globalThis, {
    useRoute: () => ({ params: { entity: widget.plural, id: routeId } }),
    useRouter: () => ({ push: (to: string) => pushes.push(to) }),
    useAdminRegistry: () => ({
      adminEntities: [widget],
      adminEntityMap: { [widget.key]: widget },
      adminEntityByPlural: { [widget.plural]: widget },
      adminFieldDefs: { [widget.key]: widget.fields },
    }),
    useTransport: () => transport,
    useAdminEntity,
    initEditFormData,
    toUpdateInput,
  })
}

function mountEditPage() {
  return mount(EditPage, {
    global: {
      components: { NuxtLink: NuxtLinkStub, AdminFormField },
    },
  })
}

/** A transport whose read returns `record` and whose update records its calls. */
function scriptedTransport(record: Rec | null) {
  const updates: { id: string; input: Rec }[] = []
  transport = {
    widgetGetById: async () => record,
    widgetUpdate: async (id, input) => {
      updates.push({ id: id as string, input: input as Rec })
      return { ...record, ...(input as Rec) }
    },
  }
  return updates
}

beforeEach(() => {
  pushes.length = 0
  stubApp()
})

describe('edit page when the record cannot be loaded', () => {
  test('a failed fetch shows the transport error instead of a blank editable form', async () => {
    routeId = 'does-not-exist'
    transport = {
      widgetGetById: async () => {
        throw new Error('404 Not Found')
      },
    }

    const wrapper = mountEditPage()
    await flushPromises()

    expect(wrapper.find('form').exists()).toBe(false)
    expect(wrapper.text()).toContain('404 Not Found')
  })

  test('a fetch that resolves to nothing says so instead of rendering a form', async () => {
    routeId = 'gone'
    scriptedTransport(null)

    const wrapper = mountEditPage()
    await flushPromises()

    expect(wrapper.find('form').exists()).toBe(false)
    expect(wrapper.text().toLowerCase()).toContain('not found')
  })
})

describe('edit page update payload', () => {
  test('untouched null number/enum fields are sent as null, never the empty string', async () => {
    routeId = 'w1'
    const updates = scriptedTransport({
      id: 'w1',
      label: 'Widget one',
      rating: null,
      notes: null,
      completion_kind: null,
      active: null,
    })

    const wrapper = mountEditPage()
    await flushPromises()

    await wrapper.find('form').trigger('submit.prevent')
    await flushPromises()

    expect(updates).toHaveLength(1)
    expect(updates[0]?.input).toEqual({
      label: 'Widget one',
      rating: null,
      notes: null,
      completion_kind: null,
      active: null,
    })
  })

  test('clearing the rating reaches the wire as an explicit null, not an absent key', async () => {
    routeId = 'w1'
    const updates = scriptedTransport({ id: 'w1', label: 'Widget one', rating: 4, notes: 'x' })

    const wrapper = mountEditPage()
    await flushPromises()

    const ratingInput = wrapper.find('input[type="number"]')
    await ratingInput.setValue('')
    await wrapper.find('form').trigger('submit.prevent')
    await flushPromises()

    expect(updates).toHaveLength(1)
    const wire = JSON.parse(JSON.stringify(updates[0]?.input)) as Rec
    expect('rating' in wire).toBe(true)
    expect(wire.rating).toBeNull()
  })
})

describe('edit page nullable boolean field', () => {
  test('a stored null boolean, saved untouched, reaches the wire as null — never false', async () => {
    routeId = 'w1'
    const updates = scriptedTransport({ id: 'w1', label: 'Widget one', rating: 1, notes: 'x', active: null })

    const wrapper = mountEditPage()
    await flushPromises()

    const checkbox = wrapper.find('input[type="checkbox"]')
    expect((checkbox.element as HTMLInputElement).checked).toBe(false) // null renders unchecked, same as false

    await wrapper.find('form').trigger('submit.prevent')
    await flushPromises()

    expect(updates).toHaveLength(1)
    expect(updates[0]?.input.active).toBeNull()
  })

  test('clicking the checkbox on a null boolean sends an explicit true', async () => {
    routeId = 'w1'
    const updates = scriptedTransport({ id: 'w1', label: 'Widget one', rating: 1, notes: 'x', active: null })

    const wrapper = mountEditPage()
    await flushPromises()

    await wrapper.find('input[type="checkbox"]').setValue(true)
    await wrapper.find('form').trigger('submit.prevent')
    await flushPromises()

    expect(updates).toHaveLength(1)
    expect(updates[0]?.input.active).toBe(true)
  })
})

describe('edit page navigation uses an encoded id', () => {
  test('saving navigates to the detail route with the id percent-encoded', async () => {
    routeId = 'a/b'
    scriptedTransport({ id: 'a/b', label: 'Slashy', rating: 1, notes: 'x' })

    const wrapper = mountEditPage()
    await flushPromises()

    await wrapper.find('form').trigger('submit.prevent')
    await flushPromises()

    expect(pushes).toEqual(['/admin/widgets/a%2Fb'])
  })

  test('the cancel link points at the encoded detail route', async () => {
    routeId = 'a/b'
    scriptedTransport({ id: 'a/b', label: 'Slashy', rating: 1, notes: 'x' })

    const wrapper = mountEditPage()
    await flushPromises()

    const cancelLink = wrapper.findAll('a').find((a) => a.text() === 'Cancel')
    expect(cancelLink?.attributes('href')).toBe('/admin/widgets/a%2Fb')
  })
})
