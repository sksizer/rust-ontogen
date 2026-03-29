import { createHttpTransport } from '~/generated/transport'
import type { Transport } from '~/generated/transport'

export type { Transport }

let cachedTransport: Transport | null = null

export function useTransport(): Transport {
  if (cachedTransport) return cachedTransport
  cachedTransport = createHttpTransport()
  return cachedTransport
}
