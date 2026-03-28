<script setup lang="ts">
// The generated transport client will be at ~/generated/transport.ts
// once `cargo build` runs in src-tauri/ and triggers ontogen.
//
// Example usage:
//   import { createIpcTransport } from '~/generated/transport'
//   const transport = createIpcTransport()
//   const exercises = await transport.listExercises()
</script>

<template>
  <div style="padding: 2rem; font-family: system-ui">
    <h1>Iron Log</h1>
    <p>Weight lifting tracker — ontogen example project.</p>
    <p>
      Run <code>cargo build</code> in <code>src-tauri/</code> to generate the
      full persistence + API + transport stack from the schema entities.
    </p>
  </div>
</template>
