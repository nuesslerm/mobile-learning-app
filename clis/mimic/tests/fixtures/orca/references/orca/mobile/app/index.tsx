import { RENAMED_METHOD } from '../../src/shared/names'
import { FILE_CAPABILITIES } from '../../src/shared/caps'
import { relayResume } from '../src/transport/relay'

export default function Home() {
  client.sendRequest('reached.allowed')
  client.sendRequest(RENAMED_METHOD)
  client.subscribe('reached.stream')
  client.sendRequest('reached.offAllowlist')
  client.sendRequest('reached.flagged')
  client.sendRequest(pickedMethod)
  supports(FILE_CAPABILITIES)
  return <Text>It's {relayResume()}</Text>
}
