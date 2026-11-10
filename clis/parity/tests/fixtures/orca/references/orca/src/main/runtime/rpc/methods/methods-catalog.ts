import { RENAMED_METHOD, SAVE_METHODS } from '../../../../shared/names'

export const FIXTURE_METHODS = [
  defineMethod({
    name: 'reached.allowed',
    permission: 'workspace',
    params: null,
    handler: () => ({ name: 'not.a.method' })
  }),
  defineMethod({ name: RENAMED_METHOD, params: null, handler }),
  defineMethod({ name: SAVE_METHODS.begin, params: null, handler }),
  defineStreamingMethod({ name: 'reached.stream', params: null, handler }),
  defineMethod({ name: 'reached.offAllowlist', params: null, handler }),
  defineMethod({ name: 'reached.flagged', params: null, handler }),
  defineMethod({ name: 'unreached.allowed', params: null, handler }),
  defineMethod({ name: 'unreached.capability', params: null, handler })
]
