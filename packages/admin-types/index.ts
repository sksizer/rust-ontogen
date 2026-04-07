// @ontogen/admin-types — Framework-agnostic type definitions for the ontogen admin registry.
// These types are shared between the admin generator (Rust), UI layers (Nuxt, React, etc.),
// and consuming applications.

export type FieldType =
  | 'string'
  | 'text'
  | 'number'
  | 'boolean'
  | 'enum'
  | 'string-array'
  | 'relation'
  | 'relation-array'

export interface AdminFieldDef {
  key: string
  label: string
  type: FieldType
  required?: boolean
  /** For 'enum' fields */
  enumValues?: string[]
  /** For 'relation' / 'relation-array' fields — target entity key */
  relationTo?: string
  /** Show in list table columns */
  showInTable?: boolean
  /** Show in detail view */
  showInDetail?: boolean
  /** Show in create/edit form */
  showInForm?: boolean
  /** Is this the primary ID field (not editable on update) */
  isId?: boolean
  /** Render as markdown body */
  isBody?: boolean
  /** Read-only (e.g. created_at) */
  readOnly?: boolean
}

export interface AdminEntityConfig {
  key: string
  plural: string
  label: string
  pluralLabel: string
  idType: 'string'
  listMethod: string
  getMethod: string
  createMethod: string
  updateMethod: string
  deleteMethod: string
  returnType: string
  createInputType: string
  updateInputType: string
  /** Whether the list endpoint returns paginated results */
  paginated?: boolean
  /** Default page size for paginated list queries */
  defaultLimit?: number
  /** Maximum allowed page size for paginated list queries */
  maxLimit?: number
  fields: AdminFieldDef[]
}
