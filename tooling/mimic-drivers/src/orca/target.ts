// Which Orca host the scripted phone pairs with: the owner's pairing offer (env, a secret: never in a case input or a cassette),
// a host's explicit endpoint, token and public key, or an orcad provisioned at the pin (the VM).
import { loadPairing } from './phone.ts'

export type Target = { endpoint: string; deviceToken: string; serverPublicKeyB64: string }

export const TARGET_ENV = 'ORCA_PAIRING_URI, or ORCA_ENDPOINT with ORCA_DEVICE_TOKEN and ORCA_SERVER_PUBLIC_KEY_B64'

export async function targetFromPairingUri(uri: string): Promise<Target> {
  const offer = (await loadPairing()).parsePairingCode(uri)
  if (!offer) throw new Error('the pairing offer is not an Orca pairing code')
  return { endpoint: offer.endpoint, deviceToken: offer.deviceToken, serverPublicKeyB64: offer.publicKeyB64 }
}

/** The target the environment names, or null when it names none. */
export async function targetFromEnv(env: NodeJS.ProcessEnv): Promise<Target | null> {
  if (env.ORCA_PAIRING_URI) return targetFromPairingUri(env.ORCA_PAIRING_URI)
  const { ORCA_ENDPOINT: endpoint, ORCA_DEVICE_TOKEN: deviceToken, ORCA_SERVER_PUBLIC_KEY_B64: serverPublicKeyB64 } = env
  if (!endpoint && !deviceToken && !serverPublicKeyB64) return null
  if (!endpoint || !deviceToken || !serverPublicKeyB64) throw new Error('ORCA_ENDPOINT, ORCA_DEVICE_TOKEN and ORCA_SERVER_PUBLIC_KEY_B64 go together')
  return { endpoint, deviceToken, serverPublicKeyB64 }
}
