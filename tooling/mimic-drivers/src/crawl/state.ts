// Which crawl cases can change what the target answers next. A recording runs the others first and many at once; these it runs in
// chunks on a target that is reset between them (tag `state:write`, wf mimic record --help). An input the schema rejects never reaches
// a handler, so only valid and boundary inputs of an endpoint that is not a read are counted.
const READ = /(^|[.:_-])(get|list|read|subscribe|status|stat|search|describe|exists|info|snapshot|state|metadata|ping|version|resolve|diff|log|show|peek|check|fetch|find|query|probe|count|tree|history|preview|poll|watch|summary|detect|validate)/i

export const TAG_WRITE = 'state:write'

// A rejected input can still touch shared state (a command id is remembered after a rejection, a revoke ends other sessions, a
// subscription sees what runs beside it), so a row that says so is a write at every class.
export function stateTags(inputClass: string, name: string, httpMethod?: string, always = false): string[] {
  if (always) return [TAG_WRITE]
  if (inputClass !== 'valid' && inputClass !== 'boundary') return []
  const reads = httpMethod ? httpMethod === 'GET' || httpMethod === 'HEAD' : READ.test(name)
  return reads ? [] : [TAG_WRITE]
}
