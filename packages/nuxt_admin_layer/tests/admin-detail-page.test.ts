/**
 * The entity detail page mounted for real: the defect (building the edit
 * route from the raw route id instead of the percent-encoded one) lives in
 * navigateToEdit's setup wiring, so this is exercised through the component
 * rather than a pure function.
 */
import type { AdminEntityConfig } from '@ontogen/admin-types'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, test } from 'vitest'
import { defineComponent, h } from 'vue'

import { useAdminEntity } from '../app/composables/useAdminEntity'
import DetailPage from '../app/pages/admin/[entity]/[id]/index.vue'

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
  // showInDetail intentionally omitted: keeps detailFields empty so the
  // template's formatFieldValue() branch (an unrelated Nuxt auto-import,
  // not stubbed by these tests) never renders.
  fields: [{ key: 'id', label: 'Id', type: 'string', isId: true }],
}

const NuxtLinkStub = defineComponent({
  name: 'NuxtLink',
  props: { to: { type: String, default: '' } },
  setup(props, { slots }) {
    return () => h('a', { href: props.to }, slots.default?.())
  },
})

const pushes: string[] = []

beforeEach(() => {
  pushes.length = 0
  Object.assign(globalThis, {
    useRoute: () => ({ params: { entity: widget.plural, id: 'a/b' } }),
    useRouter: () => ({ push: (to: string) => pushes.push(to) }),
    useAdminRegistry: () => ({
      adminEntities: [widget],
      adminEntityMap: { [widget.key]: widget },
      adminEntityByPlural: { [widget.plural]: widget },
      adminFieldDefs: { [widget.key]: widget.fields },
    }),
    useTransport: () => ({
      widgetGetById: async () => ({ id: 'a/b' }),
    }),
    useAdminEntity,
  })
})

describe('detail page edit navigation', () => {
  test('navigating to Edit percent-encodes an id containing "/"', async () => {
    const wrapper = mount(DetailPage, {
      global: {
        components: { NuxtLink: NuxtLinkStub, AdminRelationTables: { template: '<div />' } },
      },
    })
    await flushPromises()

    await wrapper.find('button').trigger('click') // the header's "Edit" button is the first button

    expect(pushes).toEqual(['/admin/widgets/a%2Fb/edit'])
  })
})
