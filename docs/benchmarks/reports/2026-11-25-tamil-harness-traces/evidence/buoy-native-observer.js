// Expo entry used for the local development-bundle measurement.
require('./index')
const { BenchmarkRecorder } = require('@buoy-gg/perf-monitor')

const endpoint = 'http://127.0.0.1:18651'
let lastCommand
let polling = false

async function poll() {
  if (polling) return
  polling = true
  try {
    const response = await fetch(endpoint + '/control')
    const { id, action } = await response.json()
    if (!id || lastCommand === id) return
    lastCommand = id

    if (action === 'start') {
      BenchmarkRecorder.start({ name: 'Tamil simulator', source: 'manual' }, { captureRenders: false })
    }
    const report = action === 'stop' ? BenchmarkRecorder.stop() : null
    await fetch(endpoint + '/ack', { method: 'POST', body: JSON.stringify({ id, report }) })
  } finally {
    polling = false
  }
}

setInterval(() => { void poll() }, 250)
