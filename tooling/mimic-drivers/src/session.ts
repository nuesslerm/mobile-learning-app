// One real server for many interactions. A `Launch` runs `use` against a server it starts and stops; `shared` keeps that server up
// between calls (the start costs seconds, an interaction milliseconds) and starts a new one only on `reset()`, the driver's state reset.
export type Launch<T> = <R>(use: (server: T) => Promise<R>) => Promise<R>

export type Shared<T> = {
  /** The running server, started on first use. `generation` counts the starts, so state taken from one server is not used on the next. */
  get(): Promise<{ server: T; generation: number }>
  reset(): Promise<void>
  close(): Promise<void>
}

type Held<T> = { server: T; stop: () => Promise<void> }

function hold<T>(launch: Launch<T>): Promise<Held<T>> {
  return new Promise((resolve, reject) => {
    let release!: () => void
    const released = new Promise<void>((r) => (release = r))
    const finished = launch(async (server) => {
      resolve({ server, stop: async () => { release(); await finished.catch(() => {}) } })
      await released
    })
    finished.catch(reject)
  })
}

export function shared<T>(launch: Launch<T>): Shared<T> {
  let held: Promise<Held<T>> | undefined
  let generation = 0
  const stop = async () => {
    const h = held
    held = undefined
    if (h) await (await h.catch(() => undefined))?.stop()
  }
  return {
    async get() {
      if (!held) {
        generation += 1
        held = hold(launch)
      }
      try {
        return { server: (await held).server, generation }
      } catch (e) {
        held = undefined
        throw e
      }
    },
    reset: stop,
    close: stop,
  }
}
