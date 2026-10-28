// Filling a form from a sentence in the online version (D-080). The models are downloaded only when the user clicks
// Download: from fixed addresses (pinned revisions / npm version), each file's SHA-256 checked, then kept in this
// browser's storage (Cache "ai-models-v1", kept across pdit updates). pdit's service worker (sw.js) serves them to
// leafmind under ai-models/. PDFs never leave the computer: only these files are fetched.
// Served next to leafmind's files in assets/ (fill_cmd.rs), though they sit in leafmind-fill/ in the source.
import { loadFill } from "./leafmind-fill-web.js";

const CACHE = "ai-models-v1";
const BASE = new URL("ai-models/", document.baseURI).href;
const HF = "https://huggingface.co";
const ORT = "https://cdn.jsdelivr.net/npm/onnxruntime-web@1.30.0/dist";

// [path under ai-models/, url, size, sha256]. gte and Needle as leafmind v0.9.0's scripts/fetch-qa-models.sh pins
// them; ONNX Runtime Web 1.30.0's WebGPU build (the files its ort.webgpu.min.mjs loads).
const FILES = [
  ["ort/ort.webgpu.min.mjs", `${ORT}/ort.webgpu.min.mjs`, 66349,
    "3dffff71811bc13a3a3d9591c57ca5ee5735b8f5f3eaf789510b02bb040ae3a0"],
  ["ort/ort-wasm-simd-threaded.asyncify.mjs", `${ORT}/ort-wasm-simd-threaded.asyncify.mjs`, 53057,
    "3d1c85995364bb643302fc6fd877a0c3ba5ae72401815e0f24828a53d9191e28"],
  ["ort/ort-wasm-simd-threaded.asyncify.wasm", `${ORT}/ort-wasm-simd-threaded.asyncify.wasm`, 26781914,
    "39f9f0894d478800487ed9f7dbe92618498db320cf55c8e3d89adff8dce658da"],
  ["gte-embed/model_q4f16.onnx",
    `${HF}/litoo13/leafmind-gte-multilingual-4lang/resolve/a789eef86239fee01ba034413e5af09291f28506/gte-embed/model_q4f16.onnx`,
    176633173, "aa9ae652c7f6effbfc7aa57465f6072256926c944922dee3845cec456168864f"],
  ["gte-embed/tokenizer.json",
    `${HF}/litoo13/leafmind-gte-multilingual-4lang/resolve/a789eef86239fee01ba034413e5af09291f28506/gte-embed/tokenizer.json`,
    2788722, "c1bafc0e9c74e6cefcd032df267ce8603c7a288a771e517d9c8823e0a7de8a37"],
  ["needle/needle.js",
    `${HF}/Cactus-Compute/needle3/resolve/d6c7d6a2e9655505e0f8e23339356dd1705900eb/wasm/needle.js`,
    62082, "a675c66f506a1b6f922d852c0cc0b82a843ce9d0fab6a80a408106f516df126f"],
  ["needle/needle.wasm",
    `${HF}/Cactus-Compute/needle3/resolve/d6c7d6a2e9655505e0f8e23339356dd1705900eb/wasm/needle.wasm`,
    930545, "7b73d09c17923bf81d6bdb82e19b3c343b8994f83d512ddec52eec9f6b029802"],
  ["fill/fill.cact",
    `${HF}/litoo13/leafmind-fill-needle/resolve/c52f3d0a11f38e6a01d094090501a35c90f2882b/fill.cact`,
    63437076, "6198a4579107fc6b0546816a547f6c8594979ba2ebe33a4a4b2c8643536c35fd"],
];

const TYPES = { mjs: "text/javascript", js: "text/javascript", wasm: "application/wasm", json: "application/json" };

/** {needed, total}: bytes still to download, and all of them. */
export async function status() {
  const cache = await caches.open(CACHE);
  let needed = 0;
  for (const [path, , size] of FILES) if (!(await cache.match(BASE + path))) needed += size;
  return { needed, total: FILES.reduce((n, f) => n + f[2], 0) };
}

let abort = null;

/** Downloads the missing files; onProgress(done, total). Rejects with "cancelled" after cancel(). */
export async function download(onProgress) {
  abort = new AbortController();
  const cache = await caches.open(CACHE);
  const total = FILES.reduce((n, f) => n + f[2], 0);
  let done = 0;
  try {
    for (const [path, url, size, sha256] of FILES) {
      if (await cache.match(BASE + path)) {
        done += size;
        onProgress(done, total);
        continue;
      }
      const response = await fetch(url, { signal: abort.signal, cache: "no-store" });
      if (!response.ok) throw new Error(`could not download ${path} (${response.status})`);
      const reader = response.body.getReader();
      const chunks = [];
      for (;;) {
        const { done: end, value } = await reader.read();
        if (end) break;
        chunks.push(value);
        done += value.length;
        onProgress(done, total);
      }
      const blob = new Blob(chunks);
      const digest = await crypto.subtle.digest("SHA-256", await blob.arrayBuffer());
      const hex = [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
      if (blob.size !== size || hex !== sha256) throw new Error(`${path} did not match its checksum`);
      const type = TYPES[path.split(".").pop()] ?? "application/octet-stream";
      await cache.put(BASE + path, new Response(blob, { headers: { "Content-Type": type } }));
    }
  } catch (e) {
    throw abort.signal.aborted ? "cancelled" : String(e?.message ?? e);
  } finally {
    abort = null;
  }
}

export function cancel() {
  abort?.abort();
}

let fill = null;
let form = null;

async function engine() {
  if (!navigator.serviceWorker?.controller) throw "pdit's offline files aren't active yet: reload the page once";
  fill ??= loadFill({
    ortUrl: BASE + "ort/",
    embedderUrl: BASE + "gte-embed/",
    needleUrl: BASE + "needle/",
    modelUrl: BASE + "fill/fill.cact",
  }).catch((e) => {
    fill = null;
    throw e;
  });
  return fill;
}

/** Reads the form: the JSON pdit's desktop app gets too ({fields, pages}, words as [text, box]). */
export async function prepare(json) {
  const f = await engine();
  const { fields, pages } = JSON.parse(json);
  const next = await f.prepare(
    fields.map((x) => ({ ...x, readOnly: x.read_only, tooltip: x.tooltip ?? undefined })),
    pages.map((p) => ({ page: p.page, words: p.words.map(([text, bounds]) => ({ text, bounds })) })),
  );
  if (form) f.closeForm(form).catch(() => {});
  form = next;
}

/** The proposal, in the desktop app's shape (fill.rs Reply). */
export async function propose(sentence, today) {
  const f = await engine();
  if (!form) throw "the form has not been read yet";
  const p = await f.propose(form, sentence, today);
  if (p.kind !== "actions") return p;
  return {
    kind: "actions",
    actions: p.actions.map(({ field, change, alternatives }) => ({
      field,
      change: change.kind,
      value: change.value ?? change.option ?? null,
      checked: change.checked ?? null,
      from: change.field ?? null,
      alternatives,
    })),
  };
}
