import { ref, computed, shallowRef } from 'vue'
import type {
  SearchRequest,
  SearchEvent,
  SearchState,
  DoneKind,
  SearchResult,
  Metrics,
} from '@/types/worker'
import {
  artifactFlavor,
  wasmThreadsAvailable,
  WASM_THREADS_ERROR,
  supportsSimd128,
  type ArtifactFlavor,
} from '@/lib/wasm-runtime'

const STATE_LABELS: Record<SearchState, string> = {
  idle: 'Idle',
  preparing: 'Preparing',
  running: 'Running',
  found: 'Found',
  stopped: 'Stopped',
  exhausted: 'Exhausted',
  error: 'Error',
}

export function useSearchWorker() {
  const state = ref<SearchState>('idle')
  const stateLabel = computed(() => STATE_LABELS[state.value])
  const activeBackend = ref('No backend')
  const device = ref('No device')
  const metrics = shallowRef<Metrics>({
    processed: 0n,
    evaluated: 0n,
    maxIndex: 0n,
    elapsedMs: 0,
  })
  const difficulty = ref(0)
  const progressPercent = computed(() => {
    const m = metrics.value
    if (m.maxIndex === 0n) return 0
    return Number((m.processed * 10000n) / m.maxIndex) / 100
  })

  const hitProbability = computed(() => {
    const bits = difficulty.value
    if (bits === 0 || bits > 64) return 0
    const attempts = Number(metrics.value.evaluated)
    if (attempts === 0) return 0
    const workFactor = Math.pow(2, bits)
    return 1 - Math.exp(-attempts / workFactor)
  })

  const expectedSeconds = computed(() => {
    const bits = difficulty.value
    if (bits === 0 || bits > 64) return 0
    const attempts = Number(metrics.value.evaluated)
    const elapsedMs = metrics.value.elapsedMs
    if (attempts === 0 || elapsedMs <= 0) return 0
    const rate = attempts / (elapsedMs / 1000)
    return Math.pow(2, bits) / rate
  })

  const fallbacks = ref<string[]>([])
  const runtimeMessage = ref('Configure and run a search.')
  const result = shallowRef<SearchResult | null>(null)
  const doneKind = ref<DoneKind | null>(null)
  const running = ref(false)

  let worker: Worker | undefined
  let workerKey: string | undefined
  let workerReady = false
  let pendingRequest: SearchRequest | undefined
  let currentRequest: SearchRequest | undefined
  let lastPaint = 0

  const simdSupported = supportsSimd128()
  const detectedThreads = Math.min(256, Math.max(1, navigator.hardwareConcurrency || 1))

  prewarm()
  async function prewarm() {
    if (!wasmThreadsAvailable() || worker) return
    const flavor = artifactFlavor('auto', simdSupported)
    const key = `${flavor}:${detectedThreads}`
    const request: SearchRequest = {
      commit: 0,
      nodeSuffix: 0,
      difficulty: 20,
      maxIndex: 1n << 58n,
      backend: 'auto',
      threads: detectedThreads,
      debug: false,
    }
    let warmed: Worker | undefined
    try {
      warmed = spawnWorker(request, flavor, key)
      await waitForReady(5000)
      worker = warmed
      workerKey = key
    } catch {
      warmed?.terminate()
      worker = undefined
      workerKey = undefined
      workerReady = false
    }
  }
  function waitForReady(timeoutMs: number) {
    return new Promise<void>((resolve, reject) => {
      if (workerReady) return resolve()
      const started = performance.now()
      const timer = setInterval(() => {
        if (workerReady) {
          clearInterval(timer)
          resolve()
        } else if (performance.now() - started > timeoutMs) {
          clearInterval(timer)
          reject(new Error('prewarm timeout'))
        }
      }, 50)
    })
  }

  function start(request: SearchRequest) {
    currentRequest = request
    difficulty.value = request.difficulty
    resetState()
    running.value = true
    state.value = 'preparing'
    runtimeMessage.value = 'Loading compute modules…'
    startWorker(request)
  }

  function stop() {
    if (!workerReady) {
      worker?.terminate()
      worker = undefined
      workerKey = undefined
      workerReady = false
      pendingRequest = undefined
      running.value = false
      state.value = 'stopped'
      runtimeMessage.value = 'Stopped before preparation completed.'
      return
    }
    worker?.postMessage({ type: 'stop' })
    runtimeMessage.value = 'Stopping after current batch…'
  }

  function dispose() {
    worker?.terminate()
    worker = undefined
  }

  function startWorker(request: SearchRequest, forcedFlavor?: ArtifactFlavor) {
    const flavor = forcedFlavor ?? artifactFlavor(request.backend, simdSupported)
    const key = `${flavor}:${request.threads}`
    pendingRequest = request

    if (worker && workerKey === key) {
      if (workerReady) worker.postMessage({ type: 'start', request })
      return
    }

    worker?.terminate()
    workerReady = false
    workerKey = key

    const base = import.meta.env.BASE_URL
    const params = new URLSearchParams({
      flavor,
      threads: String(request.threads),
    })
    if (request.debug) params.set('debug', '1')

    worker = spawnWorker(request, flavor, key, base, params)
  }

  function spawnWorker(
    request: SearchRequest,
    flavor: ArtifactFlavor,
    key: string,
    base = import.meta.env.BASE_URL,
    params?: URLSearchParams,
  ): Worker {
    const query = params ?? new URLSearchParams({ flavor, threads: String(request.threads) })
    const next = new Worker(`${base}search-worker.js?${query}`, {
      type: 'module',
    })
    next.onmessage = (e: MessageEvent<SearchEvent>) => handleMessage(e.data)
    next.onerror = (e) => {
      handleMessage({
        type: 'loaderError',
        flavor,
        message: e.message || 'Worker startup failed before a detailed error was reported.',
      })
    }
    workerKey = key
    return next
  }

  function handleMessage(data: SearchEvent) {
    if (
      data.type === 'loaderError' &&
      data.flavor === 'accelerated' &&
      pendingRequest &&
      ['auto', 'scalar'].includes(pendingRequest.backend)
    ) {
      worker?.terminate()
      worker = undefined
      workerKey = undefined
      workerReady = false
      startWorker({ ...pendingRequest, acceleratedLoadFailed: true }, 'scalar')
      return
    }

    if (data.type === 'loaderError') {
      worker?.terminate()
      worker = undefined
      workerKey = undefined
      workerReady = false
      data = {
        type: 'error',
        message: `${data.flavor} WASM failed to load: ${data.message}`,
      }
    }

    switch (data.type) {
      case 'ready':
        if (!workerReady) {
          workerReady = true
          if (pendingRequest) {
            worker?.postMessage({ type: 'start', request: pendingRequest })
          }
        }
        break

      case 'prepared':
        activeBackend.value = data.backend
        fallbacks.value = data.fallbacks
        state.value = 'running'
        runtimeMessage.value = 'Searching…'
        break

      case 'device':
        device.value = data.name
        break

      case 'progress':
        if (performance.now() - lastPaint >= 50) {
          metrics.value = {
            processed: BigInt(data.processed),
            evaluated: BigInt(data.evaluated),
            maxIndex: BigInt(data.maxIndex),
            elapsedMs: data.elapsedMs,
          }
          lastPaint = performance.now()
        }
        break

      case 'done':
        finish(data)
        break

      case 'error':
        running.value = false
        state.value = 'error'
        runtimeMessage.value = data.message
        break
    }
  }

  function finish(data: Extract<SearchEvent, { type: 'done' }>) {
    running.value = false
    metrics.value = {
      processed: BigInt(data.processed),
      evaluated: BigInt(data.evaluated),
      maxIndex: currentRequest?.maxIndex ?? 0n,
      elapsedMs: data.elapsedMs,
    }
    fallbacks.value = data.fallbacks

    if (data.kind === 'found') {
      state.value = 'found'
      doneKind.value = 'found'
      result.value = {
        uuid: data.uuid ?? '',
        hash: data.hash ?? '',
        index: BigInt(data.index ?? '0'),
      }
      runtimeMessage.value = 'Lowest match in verified prefix.'
    } else {
      doneKind.value = data.kind
      state.value = data.kind
      result.value = null
      runtimeMessage.value = data.kind === 'stopped' ? 'Search stopped.' : 'Range fully evaluated.'
    }
  }

  function resetState() {
    lastPaint = 0
    metrics.value = { processed: 0n, evaluated: 0n, maxIndex: 0n, elapsedMs: 0 }
    fallbacks.value = []
    result.value = null
    doneKind.value = null
    activeBackend.value = 'Preparing'
    device.value = 'No device'
  }

  return {
    state,
    stateLabel,
    activeBackend,
    device,
    metrics,
    progressPercent,
    hitProbability,
    expectedSeconds,
    fallbacks,
    runtimeMessage,
    result,
    doneKind,
    running,
    simdSupported,
    detectedThreads,
    start,
    stop,
    dispose,
    wasmThreadsAvailable,
    WASM_THREADS_ERROR,
  }
}
