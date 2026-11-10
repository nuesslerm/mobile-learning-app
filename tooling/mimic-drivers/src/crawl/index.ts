// `crawl <orca|t3> [--runtime <dir>]`: the endpoint inventory and request corpus of the pinned reference, as one `mimic-crawl-report/1` object.
import { orcaCrawl } from './orca.ts'
import { t3Crawl } from './t3.ts'
import type { Report } from './types.ts'

export async function crawl(app: 'orca' | 't3', args: string[]): Promise<Report> {
  const at = args.indexOf('--runtime')
  const runtime = at >= 0 ? args[at + 1] : undefined
  return app === 'orca' ? orcaCrawl() : t3Crawl({ runtime })
}
