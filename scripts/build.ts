#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { artifactFlavor, wasmThreadsAvailable } from '../web/public/wasm-runtime.js'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const webDir = path.join(root, 'web')
const pkgDir = path.join(webDir, 'public', 'pkg')
const wasmTarget = 'wasm32-unknown-unknown'
const wasmLib = 'xray-qbrute-wasm'
const wasmOut = path.join(root, 'target', wasmTarget, 'release', 'xray_qbrute.wasm')

const flavors = [
  { name: 'scalar', features: 'wasm-controller' },
  { name: 'accelerated', features: 'wasm-webgpu,wasm-simd' },
]

const RAYON_HELPER = `/*
 * Derived from wasm-bindgen-rayon 1.3.0 workerHelpers.no-bundler.js.
 * Copyright 2022 Google Inc. Licensed under Apache-2.0.
 */

function waitForReady(worker) {
  return new Promise((resolve, reject) => {
    const onMessage = ({ data }) => {
      if (data?.type !== "wasm_bindgen_worker_ready") return;
      worker.removeEventListener("message", onMessage);
      worker.removeEventListener("error", onError);
      resolve();
    };
    const onError = (event) => {
      worker.removeEventListener("message", onMessage);
      worker.removeEventListener("error", onError);
      reject(new Error(event.message || "Rayon worker failed to initialize"));
    };
    worker.addEventListener("message", onMessage);
    worker.addEventListener("error", onError);
  });
}

let workers;

export async function startWorkers(module, memory, builder) {
  const threadCount = builder.numThreads();
  if (threadCount === 0) throw new Error("num_threads must be > 0");

  const mainUrl = new URL(builder.mainJS());
  const workerGlueResponse = await fetch(new URL("xray_qbrute.worker.js", mainUrl));
  if (!workerGlueResponse.ok) {
    throw new Error(\`Rayon worker glue request failed: \${workerGlueResponse.status}\`);
  }
  const workerModuleUrl = URL.createObjectURL(new Blob(
    [await workerGlueResponse.text()],
    { type: "text/javascript" },
  ));
  const bootstrapUrl = URL.createObjectURL(new Blob([
    \`self.addEventListener("message", async function initialize({ data }) {
      if (data?.type !== "wasm_bindgen_worker_init") return;
      self.removeEventListener("message", initialize);
      const pkg = await import(data.workerModuleUrl);
      await pkg.default({ module_or_path: data.module, memory: data.memory });
      self.postMessage({ type: "wasm_bindgen_worker_ready" });
      pkg.wbg_rayon_start_worker(data.receiver);
    });\`,
  ], { type: "text/javascript" }));

  const workerInit = {
    type: "wasm_bindgen_worker_init",
    module,
    memory,
    receiver: builder.receiver(),
    workerModuleUrl,
  };

  try {
    workers = Array.from({ length: threadCount }, () => new Worker(bootstrapUrl, { type: "module" }));
    await Promise.all(workers.map(async (worker) => {
      const ready = waitForReady(worker);
      worker.postMessage(workerInit);
      await ready;
    }));
    builder.build();
  } finally {
    URL.revokeObjectURL(bootstrapUrl);
    URL.revokeObjectURL(workerModuleUrl);
  }
}
`

function run(command, args, opts = {}) {
  const line = [command, ...args].join(' ')
  console.log(`\n$ ${line}`)
  const result = spawnSync(command, args, {
    cwd: opts.cwd ?? root,
    stdio: 'inherit',
    shell: false,
  })
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

function buildCli() {
  run('cargo', ['build', '--release'])
}

function patchRayonPackage(directory) {
  const gluePath = path.join(directory, 'xray_qbrute.js')
  const workerGluePath = path.join(directory, 'xray_qbrute.worker.js')
  const glue = fs.readFileSync(gluePath, 'utf8')
  const workerGlue = glue.replace(
    /^import \{ startWorkers \} from '.+workerHelpers\.no-bundler\.js';$/m,
    "const startWorkers = () => { throw new Error('nested Rayon pool initialization is unavailable'); };",
  )
  if (workerGlue === glue) throw new Error(`Rayon helper import not found in ${gluePath}`)
  fs.writeFileSync(workerGluePath, workerGlue)

  const sharedHelperPath = path.join(pkgDir, 'shared', 'workerHelpers.no-bundler.js')
  if (!fs.existsSync(sharedHelperPath)) {
    fs.mkdirSync(path.dirname(sharedHelperPath), { recursive: true })
    fs.writeFileSync(sharedHelperPath, RAYON_HELPER)
  }

  const snippetDirectories = fs.readdirSync(path.join(directory, 'snippets'), {
    withFileTypes: true,
  })
  const rayonDirectory = snippetDirectories.find(
    (entry) => entry.isDirectory() && entry.name.startsWith('wasm-bindgen-rayon-'),
  )
  if (!rayonDirectory) throw new Error(`Rayon snippet directory not found in ${directory}`)

  const glueImportPath = `./snippets/${rayonDirectory.name}/src/workerHelpers.no-bundler.js`
  const sharedImportPath = '../shared/workerHelpers.no-bundler.js'
  const reGlue = glue.replace(
    new RegExp(`from '${glueImportPath.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}'`),
    `from '${sharedImportPath}'`,
  )
  if (reGlue === glue) throw new Error(`Glue import path not rewritten in ${gluePath}`)
  fs.writeFileSync(gluePath, reGlue)

  fs.rmSync(path.join(directory, 'snippets'), { recursive: true, force: true })
}

function buildWasm() {
  fs.rmSync(pkgDir, { recursive: true, force: true })
  for (const flavor of flavors) {
    run('cargo', [
      'build',
      '--release',
      '-p',
      wasmLib,
      '--lib',
      '--target',
      wasmTarget,
      '--no-default-features',
      '--features',
      flavor.features,
    ])
    run('wasm-bindgen', [
      '--target',
      'web',
      '--no-typescript',
      '--out-dir',
      path.join(pkgDir, flavor.name),
      '--out-name',
      'xray_qbrute',
      wasmOut,
    ])
    patchRayonPackage(path.join(pkgDir, flavor.name))
  }
}

function verify() {
  const webPackage = JSON.parse(fs.readFileSync(path.join(webDir, 'package.json'), 'utf8'))
  if (!webPackage.dependencies?.['coi-serviceworker']) {
    throw new Error('coi-serviceworker is not in web/package.json dependencies')
  }
  for (const flavor of flavors) {
    const directory = path.join(pkgDir, flavor.name)
    const glue = fs.readFileSync(path.join(directory, 'xray_qbrute.js'), 'utf8')
    const workerGlue = fs.readFileSync(path.join(directory, 'xray_qbrute.worker.js'), 'utf8')
    const wasm = fs.readFileSync(path.join(directory, 'xray_qbrute_bg.wasm'))
    if (!WebAssembly.validate(wasm)) throw new Error(`${flavor} WASM is invalid`)
    if (!glue.includes('initThreadPool')) throw new Error(`${flavor} thread export missing`)
    if (!glue.includes('../shared/workerHelpers.no-bundler.js')) {
      throw new Error(`${flavor} glue does not reference the shared Rayon helper`)
    }
    if (
      workerGlue.includes('import { startWorkers }') ||
      !workerGlue.includes('nested Rayon pool initialization')
    ) {
      throw new Error(`${flavor} worker glue still imports the Rayon pool helper`)
    }
  }
  const helper = fs.readFileSync(path.join(pkgDir, 'shared', 'workerHelpers.no-bundler.js'), 'utf8')
  if (helper.includes('fetch(import.meta.url)') || !helper.includes('workerModuleUrl')) {
    throw new Error('Rayon helper still downloads one bootstrap per thread')
  }
  if (
    artifactFlavor('scalar', true) !== 'accelerated' ||
    artifactFlavor('wasm-simd', true) !== 'accelerated'
  ) {
    throw new Error('SIMD-capable browsers do not reuse one artifact for Scalar and SIMD')
  }
  if (artifactFlavor('scalar', false) !== 'scalar' || artifactFlavor('auto', false) !== 'scalar') {
    throw new Error('baseline browsers do not select the scalar artifact')
  }
  if (!wasmThreadsAvailable({ crossOriginIsolated: true, SharedArrayBuffer })) {
    throw new Error('cross-origin isolated runtimes were rejected')
  }
  if (wasmThreadsAvailable({ crossOriginIsolated: false, SharedArrayBuffer })) {
    throw new Error('non-isolated runtimes were accepted for WASM threads')
  }
  console.log(
    'Browser artifacts validated; Scalar/SIMD share the accelerated package when SIMD128 is available',
  )
}

function buildWeb() {
  fs.rmSync(path.join(webDir, 'dist'), { recursive: true, force: true })
  run('node', [path.join(webDir, 'node_modules/vue-tsc/bin/vue-tsc.js'), '--noEmit'], {
    cwd: webDir,
  })
  run('node', [path.join(webDir, 'node_modules/vite/bin/vite.js'), 'build'], { cwd: webDir })
}

function buildWebAll() {
  buildWasm()
  verify()
  buildWeb()
}

const tasks = {
  cli: 'build the CLI binary (crates/cli)',
  wasm: 'build the WebAssembly packages (web/public/pkg/{scalar,accelerated})',
  web: 'build the web frontend (vue-tsc + vite)',
  'web-all': 'build the full web deliverable (wasm packages, validate, frontend)',
  verify: 'validate the built wasm packages',
  all: 'build everything (cli + web-all)',
}

function printHelp() {
  console.log('xray-qbrute build script\n')
  console.log('usage: node scripts/build.ts [task]\n')
  console.log('tasks:')
  for (const [name, description] of Object.entries(tasks)) {
    console.log(`  ${name.padEnd(9)} ${description}`)
  }
  console.log('\n(default: all)')
}

const task = process.argv[2] ?? 'all'
if (task === '--help' || task === '-h') {
  printHelp()
  process.exit(0)
}

switch (task) {
  case 'cli':
    buildCli()
    break
  case 'wasm':
    buildWasm()
    break
  case 'verify':
    verify()
    break
  case 'web':
    buildWeb()
    break
  case 'web-all':
    buildWebAll()
    break
  case 'all':
    buildCli()
    buildWebAll()
    break
  default:
    console.error(`unknown task: ${task}`)
    printHelp()
    process.exit(1)
}

console.log('\nOK')
