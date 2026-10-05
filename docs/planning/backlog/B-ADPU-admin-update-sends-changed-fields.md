---
type: backlog
schema_version: '2'
id: B-ADPU
tags:
- admin
- nuxt
- relations
- http
last_reviewed: '2026-10-05'
---

# Send only the changed fields from the admin edit form

The Nuxt admin layer's edit page builds its update with `toUpdateInput(fields, formData)` (`packages/nuxt_admin_layer/app/utils/adminFormInput.ts`, called from `app/pages/admin/[entity]/[id]/edit.vue`). `formData` is seeded from every form field of the loaded record (`initEditFormData`), and `toUpdateInput` copies all of it, turning a blank optional field into `null`. A save therefore sends every field, including each relationship list in full, whether or not the user touched it.

A markdown record can hold a dangling `many_to_many` link: a delete leaves the id behind in other records' lists (ADR 0001 amendment 5, B-DLFK). Saving such a record re-sends that list, and the HTTP server's step-8 check (JSON:API wire contract §9.1) refuses the id with `404 no_such_related_resource`. The user cannot save an unrelated edit to the record until the link is removed by other means.

This predates the fixes in PR C: the HTTP step-8 check has existed since the JSON:API wire landed.

## Proposal

- Keep the record as loaded, and have the edit page send only the fields whose form value differs from it (a deep comparison for list fields). A relationship the user did not touch is then left out of the `PATCH`, which the server reads as "leave the stored value alone" (contract §8.3).
- Keep the existing rule that a cleared optional field is sent as `null`, but only when it changed.

## What done looks like

- Saving a record that holds a dangling `many_to_many` id, after editing an unrelated field, succeeds and does not send the relationship.
- Editing a relationship list still sends the whole list.
- Vitest cases in `packages/nuxt_admin_layer/tests` (`admin-form-input.test.ts`, `admin-edit-page.test.ts`) cover an untouched list, a changed list, a cleared optional field and an untouched one.
