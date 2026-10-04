// Drives a running tasks-tracker with kitsu, a published JSON:API client,
// configured only through its documented constructor options. Nothing here
// builds a request or parses a response by hand: if the server strays from
// JSON:API, the client is what notices. `run.sh` starts the server on a
// throwaway copy of the vault, because these steps create, change and delete
// records.
import assert from 'node:assert/strict'
import Kitsu from 'kitsu'

const baseURL = process.env.BASE_URL ?? 'http://127.0.0.1:39302/api'

// The server's types and paths are already the plural, lowercase JSON:API
// names (`tasks`, `epics`), so kitsu's defaults that rewrite them for
// kitsu.app are switched off.
const api = new Kitsu({
  baseURL,
  pluralize: false,
  camelCaseTypes: false,
  resourceCase: 'none'
})

let failed = false
async function step (name, fn) {
  try {
    const detail = await fn()
    console.log(`ok   ${name}${detail ? ` (${detail})` : ''}`)
  } catch (err) {
    failed = true
    console.log(`FAIL ${name}`)
    console.log(err?.errors ? { message: err.message, errors: err.errors } : err)
  }
}

const ids = (res) => res.data.map((r) => r.id)

await step('list tasks', async () => {
  const res = await api.get('tasks')
  assert.equal(res.status, 200)
  assert.deepEqual(ids(res), ['ship-the-emitter'])
  assert.equal(res.data[0].title, 'Ship the emitter')
  assert.equal(res.meta.total, 1)
  return `meta.total=${res.meta.total}`
})

await step('get a task with include=epic,tags links the included resources', async () => {
  const res = await api.get('tasks/ship-the-emitter', { params: { include: 'epic,tags' } })
  assert.equal(res.data.id, 'ship-the-emitter')
  assert.equal(res.data.status, 'closed/done')
  assert.equal(res.data.epic.data.type, 'epics')
  assert.equal(res.data.epic.data.title, 'Markdown backend')
  assert.deepEqual(res.data.tags.data.map((t) => [t.type, t.id, t.title]), [['tags', 'codegen', 'Codegen']])
  return `epic.title=${JSON.stringify(res.data.epic.data.title)}, tags[0].title=${JSON.stringify(res.data.tags.data[0].title)}`
})

await step('create a task with an epic and two tags', async () => {
  const res = await api.create('tasks', {
    title: 'Conformance parent',
    status: 'open/ready',
    created: '2026-10-01',
    body: '## Goal\n\nBe driven by a third-party client.\n',
    epic: { data: { type: 'epics', id: 'markdown-backend' } },
    tags: { data: [{ type: 'tags', id: 'codegen' }, { type: 'tags', id: 'release' }] }
  })
  assert.equal(res.status, 201)
  assert.equal(res.data.type, 'tasks')
  assert.equal(res.data.id, 'conformance-parent')
  assert.equal(res.data.title, 'Conformance parent')
  assert.equal(res.data.status, 'open/ready')
  assert.equal(res.data.epic.data.id, 'markdown-backend')
  assert.deepEqual(res.data.tags.data.map((t) => t.id), ['codegen', 'release'])
  return `201, id=${res.data.id}`
})

await step('create a subtask with a parent', async () => {
  const res = await api.create('tasks', {
    title: 'Conformance child',
    status: 'in-progress',
    created: '2026-10-02',
    body: '',
    parent: { data: { type: 'tasks', id: 'conformance-parent' } }
  })
  assert.equal(res.status, 201)
  assert.equal(res.data.id, 'conformance-child')
  assert.equal(res.data.parent.data.id, 'conformance-parent')
  return `201, id=${res.data.id}`
})

await step('list a page with page[limit] and page[offset]', async () => {
  const res = await api.get('tasks', { params: { sort: 'id', page: { limit: 2, offset: 1 } } })
  assert.deepEqual(ids(res), ['conformance-parent', 'ship-the-emitter'])
  assert.equal(res.meta.total, 3)
  assert.equal(res.meta.limit, 2)
  assert.equal(res.meta.offset, 1)
  assert.ok(res.links.prev, 'links.prev is set on a page past the first')
  assert.equal(res.links.next, null)
  return `ids=${ids(res).join(',')}, meta.total=${res.meta.total}`
})

await step('list sorted descending by created', async () => {
  const res = await api.get('tasks', { params: { sort: '-created' } })
  assert.deepEqual(ids(res), ['conformance-child', 'conformance-parent', 'ship-the-emitter'])
  return `ids=${ids(res).join(',')}`
})

await step('list filtered by status, including each epic', async () => {
  const res = await api.get('tasks', { params: { filter: { status: 'open/ready' }, include: 'epic' } })
  assert.deepEqual(ids(res), ['conformance-parent'])
  assert.equal(res.meta.total, 1)
  assert.equal(res.data[0].epic.data.title, 'Markdown backend')
  return `ids=${ids(res).join(',')}, meta.total=${res.meta.total}`
})

await step('patch an attribute and two relationships', async () => {
  const res = await api.update('tasks', {
    id: 'conformance-parent',
    status: 'closed/done',
    epic: { data: null },
    tags: { data: [{ type: 'tags', id: 'release' }] }
  })
  assert.equal(res.status, 200)
  assert.equal(res.data.status, 'closed/done')
  assert.equal(res.data.title, 'Conformance parent', 'attributes left out of a PATCH keep their value')
  assert.equal(res.data.epic.data, undefined, 'a cleared to-one has no linked data')
  assert.deepEqual(res.data.tags.data.map((t) => t.id), ['release'])
  const again = await api.get('tasks/conformance-parent')
  assert.equal(again.data.status, 'closed/done')
  assert.deepEqual(again.data.tags.data.map((t) => t.id), ['release'])
  return `status=${again.data.status}, tags=${again.data.tags.data.map((t) => t.id).join(',')}`
})

await step('fetch a relationship: /tasks/{id}/relationships/tags', async () => {
  const res = await api.get('tasks/ship-the-emitter/relationships/tags')
  assert.equal(res.status, 200)
  assert.deepEqual(res.data.map((t) => [t.type, t.id]), [['tags', 'codegen']])
  assert.equal(res.links.related, '/api/tasks/ship-the-emitter/tags')
  return `data=${res.data.map((t) => `${t.type}/${t.id}`).join(',')}`
})

const tagIds = async (id) => (await api.get(`tasks/${id}/relationships/tags`)).data.map((t) => t.id)

await step('replace a to-many through its relationship endpoint', async () => {
  const res = await api.update('tasks/conformance-parent/relationships/tags', [{ type: 'tags', id: 'codegen' }])
  assert.equal(res.status, 204)
  const tags = await tagIds('conformance-parent')
  assert.deepEqual(tags, ['codegen'])
  return `204, tags=${tags.join(',')}`
})

await step('add to a to-many through its relationship endpoint', async () => {
  const res = await api.create('tasks/conformance-parent/relationships/tags', [{ type: 'tags', id: 'release' }])
  assert.equal(res.status, 204)
  const tags = await tagIds('conformance-parent')
  assert.deepEqual(tags, ['codegen', 'release'])
  return `204, tags=${tags.join(',')}`
})

await step('remove from a to-many through its relationship endpoint', async () => {
  const res = await api.remove('tasks/conformance-parent/relationships/tags', ['codegen'])
  assert.equal(res.status, 204)
  const tags = await tagIds('conformance-parent')
  assert.deepEqual(tags, ['release'])
  return `204, tags=${tags.join(',')}`
})

await step('fetch a related to-one: /tasks/{id}/epic', async () => {
  const res = await api.get('tasks/ship-the-emitter/epic')
  assert.equal(res.data.type, 'epics')
  assert.equal(res.data.id, 'markdown-backend')
  assert.equal(res.data.title, 'Markdown backend')
  return `epics/${res.data.id} title=${JSON.stringify(res.data.title)}`
})

await step('fetch a related to-many: /tasks/{id}/subtasks', async () => {
  const res = await api.get('tasks/conformance-parent/subtasks')
  assert.deepEqual(ids(res), ['conformance-child'])
  assert.equal(res.data[0].title, 'Conformance child')
  return `ids=${ids(res).join(',')}`
})

// kitsu's update() appends the body's id to the URL, so it can only send
// null (not a new identifier) to a to-one relationship endpoint; setting a
// to-one is covered by the creates above.
await step('clear a to-one through its relationship endpoint', async () => {
  const res = await api.update('tasks/conformance-child/relationships/parent', null)
  assert.equal(res.status, 204)
  const linkage = await api.get('tasks/conformance-child/relationships/parent')
  assert.equal(linkage.data, null)
  const subtasks = await api.get('tasks/conformance-parent/subtasks')
  assert.deepEqual(ids(subtasks), [], 'the parent no longer lists the child')
  return '204, parent=null, parent\'s subtasks=[]'
})

await step('delete a task', async () => {
  const res = await api.remove('tasks', 'conformance-child')
  assert.equal(res.status, 204)
  return '204'
})

await step('get the deleted task is a 404 with an errors[] document', async () => {
  const err = await api.get('tasks/conformance-child').then(
    () => assert.fail('expected the get to reject'),
    (e) => e
  )
  assert.equal(err.response?.status, 404)
  assert.ok(Array.isArray(err.errors), 'the client exposes the errors[] array')
  assert.equal(err.errors[0].status, '404')
  assert.equal(err.errors[0].code, 'task_not_found')
  return `404, errors[0]=${JSON.stringify(err.errors[0])}`
})

if (failed) {
  console.log('conformance: FAILED')
  process.exit(1)
}
console.log('conformance: all steps passed')
