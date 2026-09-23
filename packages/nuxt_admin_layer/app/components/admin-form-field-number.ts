// Extracted from AdminFormField.vue's number-input handler so its coercion
// rule can be unit-tested independently of Vue.
//
// A cleared number input's raw value is '', and `Number('')` is 0 — a
// non-obvious JS quirk that would turn "user cleared the field" into "user
// entered 0". Treating '' as undefined instead lets the page's own
// null-vs-absent cleanup (adminFormInput.ts's toUpdateInput/toCreateInput)
// decide what a cleared optional number means on the wire.
//
// An unparsable non-empty string (e.g. a bare '-' mid-typing, or a value set
// programmatically rather than through the browser's own number-input
// validation) coerces to NaN, which `JSON.stringify` turns into a literal
// `null` — indistinguishable from an intentional clear. Treated as undefined
// instead, same as '', so it goes through the same null-vs-absent decision
// rather than silently becoming a clear.
export function coerceNumberInput(raw: string): number | undefined {
  if (raw === '') return undefined
  const value = Number(raw)
  return Number.isNaN(value) ? undefined : value
}
