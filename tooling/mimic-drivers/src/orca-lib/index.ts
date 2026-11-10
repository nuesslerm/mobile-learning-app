import type { Driver } from '../protocol.ts'
import { toolchainLockHash } from '../lock.ts'
import { ops as agentSession } from './agent-session.ts'
import { ops as crypto } from './crypto.ts'
import { ops as mobileBundle } from './mobile-bundle.ts'
import { readPin } from './pin.ts'

export function orcaLib(): Driver {
  return { name: 'mimic-drivers/orca-lib', version: '1', protocol: 'mimic-driver/1', toolchainLockHash: toolchainLockHash(), targetVersion: readPin().version, ops: { ...mobileBundle, ...crypto, ...agentSession } }
}
