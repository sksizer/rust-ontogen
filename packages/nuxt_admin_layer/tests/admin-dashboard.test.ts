/**
 * The admin dashboard mounted for real: the defect (treating every list
 * method's result as a plain array, so a paginated entity's `{ items, total }`
 * envelope reads `.length` as undefined) lives in onMounted's counting logic,
 * so this is exercised through the component.
 */
import type { AdminEntityConfig } from '@ontogen/admin-types'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, test } from 'vitest'
import { defineComponent, h } from 'vue'

import DashboardPage from '../app/pages/admin/index.vue'
import { countFromListResult } from '../app/utils/adminListAll'

const person: AdminEntityConfig = {
  key: 'person',
  plural: 'people',
  label: 'Person',
  pluralLabel: 'People',
  idType: 'string',
  listMethod: 'personList',
  getMethod: 'personGetById',
  createMethod: 'personCreate',
  updateMethod: 'personUpdate',
  deleteMethod: 'personDelete',
  returnType: 'Person',
  createInputType: 'CreatePersonInput',
  updateInputType: 'UpdatePersonInput',
  fields: [],
}

const activity: AdminEntityConfig = {
  key: 'activity',
  plural: 'activities',
  label: 'Activity',
  pluralLabel: 'Activities',
  idType: 'string',
  listMethod: 'activityList',
  getMethod: 'activityGetById',
  createMethod: 'activityCreate',
  updateMethod: 'activityUpdate',
  deleteMethod: 'activityDelete',
  returnType: 'Activity',
  createInputType: 'CreateActivityInput',
  updateInputType: 'UpdateActivityInput',
  paginated: true,
  defaultLimit: 50,
  fields: [],
}

const NuxtLinkStub = defineComponent({
  name: 'NuxtLink',
  props: { to: { type: String, default: '' } },
  setup(props, { slots }) {
    return () => h('a', { href: props.to }, slots.default?.())
  },
})

beforeEach(() => {
  Object.assign(globalThis, {
    useAdminRegistry: () => ({
      adminEntities: [person, activity],
      adminEntityMap: { person, activity },
      adminEntityByPlural: { people: person, activities: activity },
      adminFieldDefs: { person: [], activity: [] },
    }),
    useTransport: () => ({
      personList: async () => [{ id: 'p1' }, { id: 'p2' }, { id: 'p3' }],
      activityList: async () => ({ items: Array.from({ length: 50 }, (_, i) => ({ id: `a${i}` })), total: 260 }),
    }),
    countFromListResult,
  })
})

describe('admin dashboard entity counts', () => {
  test('a paginated entity shows its true total, not "-"', async () => {
    const wrapper = mount(DashboardPage, { global: { components: { NuxtLink: NuxtLinkStub } } })
    await flushPromises()

    const tiles = wrapper.findAll('a')
    const activityTile = tiles.find((tile) => tile.text().includes('Activities'))
    const personTile = tiles.find((tile) => tile.text().includes('People'))

    expect(activityTile?.text()).toContain('260')
    expect(activityTile?.text()).not.toContain('-')
    expect(personTile?.text()).toContain('3')
  })
})
