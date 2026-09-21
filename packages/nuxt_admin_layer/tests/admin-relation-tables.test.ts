/**
 * AdminRelationTables mounted for real: the defect (fetching a paginated
 * source entity's list with no page args, then `.filter`-ing the envelope as
 * if it were the array) lives in onMounted's data-fetching, not in a
 * reducible pure function, so this is exercised through the component.
 */
import type { AdminEntityConfig, AdminFieldDef } from '@ontogen/admin-types'
import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, test } from 'vitest'
import { defineComponent, h } from 'vue'

import AdminRelationTables from '../app/components/AdminRelationTables.vue'
import { fetchAllItems } from '../app/utils/adminListAll'

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
  defaultLimit: 2,
  maxLimit: 2,
  fields: [],
}

const activityFields: AdminFieldDef[] = [
  { key: 'id', label: 'Id', type: 'string', isId: true },
  { key: 'person_id', label: 'Person', type: 'relation', relationTo: 'person' },
]

const allActivities = [
  { id: 'a1', person_id: 'p1' },
  { id: 'a2', person_id: 'other' },
  { id: 'a3', person_id: 'p1' },
]

const NuxtLinkStub = defineComponent({
  name: 'NuxtLink',
  props: { to: { type: String, default: '' } },
  setup(props, { slots }) {
    return () => h('a', { href: props.to }, slots.default?.())
  },
})

beforeEach(() => {
  const calls: unknown[][] = []
  Object.assign(globalThis, {
    useTransport: () => ({
      activityList: async (...args: unknown[]) => {
        calls.push(args)
        const [limit, offset] = (args.length > 0 ? args : [2, 0]) as [number, number]
        return { items: allActivities.slice(offset, offset + limit), total: allActivities.length }
      },
    }),
    useAdminRegistry: () => ({
      adminEntities: [person, activity],
      adminEntityMap: { person, activity },
      adminEntityByPlural: { people: person, activities: activity },
      adminFieldDefs: { person: [], activity: activityFields },
    }),
    fetchAllItems,
  })
  ;(globalThis as Record<string, unknown>).__activityListCalls = calls
})

describe('inverse relation on a paginated source entity', () => {
  test('pages through every activity instead of reporting None for a referenced person', async () => {
    const wrapper = mount(AdminRelationTables, {
      props: { entityKey: 'person', entityId: 'p1', fields: [] },
      global: { components: { NuxtLink: NuxtLinkStub } },
    })
    await flushPromises()

    expect(wrapper.text()).not.toContain('None')
    const rows = wrapper.findAll('tbody tr')
    expect(rows).toHaveLength(2)
    expect(wrapper.text()).toContain('a1')
    expect(wrapper.text()).toContain('a3')
    expect(wrapper.text()).not.toContain('a2')
  })
})
