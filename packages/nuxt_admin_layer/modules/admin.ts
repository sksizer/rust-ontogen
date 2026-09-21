// @ts-nocheck - this module runs in Nuxt's build context, not the consuming app's tsc
import { defineNuxtModule } from '@nuxt/kit'
import { join } from 'pathe'

export interface OntogenAdminOptions {
  /**
   * Absolute path (without extension) of the generated admin registry the
   * `#admin-registry` alias resolves to. Defaults to the consuming app's
   * `app/admin/generated/admin-registry`; a host whose registry is emitted
   * into a layer rather than the root app names it here.
   */
  registry?: string
}

export default defineNuxtModule<OntogenAdminOptions>({
  meta: { name: 'ontogen-admin', configKey: 'ontogenAdmin' },
  setup(options, nuxt) {
    // Point #admin-registry at the generated file (ontogen's AdminRegistry
    // generator in build.rs writes it).
    nuxt.options.alias['#admin-registry'] =
      options.registry ?? join(nuxt.options.rootDir, 'app/admin/generated/admin-registry')
  },
})
