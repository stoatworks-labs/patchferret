// PatchFerret browser glue.
//
// Loads the WASM core and drives it. Every byte of a show file stays in this
// page — there is no fetch of user data anywhere in this file, and there is no
// server to send it to. The only network request is for the .wasm module.

const KIND = { ERROR: 0, SUMMARY: 1, XML: 2, PDF: 3, WARNING: 4 };

let wasm = null;

/** Load the core. Safe to call repeatedly. */
export async function init(url = './patchferret.wasm') {
  if (wasm) return wasm;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`could not load ${url}: HTTP ${response.status}`);
  // instantiateStreaming needs the right MIME type; fall back if the host
  // serves .wasm as octet-stream.
  let result;
  try {
    result = await WebAssembly.instantiateStreaming(response.clone(), {});
  } catch {
    result = await WebAssembly.instantiate(await response.arrayBuffer(), {});
  }
  wasm = result.instance.exports;
  return wasm;
}

/** Copy a JS byte array into WASM memory. Returns [ptr, len]. */
function copyIn(bytes) {
  const ptr = wasm.pf_alloc(bytes.length);
  if (!ptr) throw new Error('WASM allocation failed');
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  return [ptr, bytes.length];
}

/**
 * Parse a show file and generate its documentation.
 *
 * @param {string} name  original file name, used for format sniffing
 * @param {Uint8Array} bytes
 * @param {string} [jobSheet]  `key: value` metadata, same syntax as the CLI
 * @param {Uint8Array} [logo]  JPEG or PNG bytes for the header
 * @returns {{summary: object|null, files: {name: string, kind: number, bytes: Uint8Array}[], error: string|null, warning: string|null}}
 */
export function process(name, bytes, jobSheet = '', logo = null) {
  if (!wasm) throw new Error('call init() first');

  const enc = new TextEncoder();
  const [namePtr, nameLen] = copyIn(enc.encode(name));
  const [dataPtr, dataLen] = copyIn(bytes);
  const [jobPtr, jobLen] = jobSheet ? copyIn(enc.encode(jobSheet)) : [0, 0];
  const [logoPtr, logoLen] = logo && logo.length ? copyIn(logo) : [0, 0];

  const resultPtr = wasm.pf_process(
    namePtr, nameLen, dataPtr, dataLen, jobPtr, jobLen, logoPtr, logoLen);
  const total = wasm.pf_result_len(resultPtr);

  // Re-read the buffer view each time: any allocation may have grown the
  // WASM memory and detached earlier views.
  const view = new DataView(wasm.memory.buffer, resultPtr, total);
  const raw = new Uint8Array(wasm.memory.buffer, resultPtr, total);

  const out = { summary: null, files: [], error: null, warning: null };
  let i = 4;
  const decoder = new TextDecoder();

  while (i < total) {
    const kind = view.getUint32(i, true); i += 4;
    const nameLength = view.getUint32(i, true); i += 4;
    const recordName = decoder.decode(raw.subarray(i, i + nameLength)); i += nameLength;
    const bodyLength = view.getUint32(i, true); i += 4;
    // Copy out: this memory is freed below.
    const body = raw.slice(i, i + bodyLength); i += bodyLength;

    if (kind === KIND.ERROR) {
      out.error = decoder.decode(body);
    } else if (kind === KIND.WARNING) {
      out.warning = decoder.decode(body);
    } else if (kind === KIND.SUMMARY) {
      try {
        out.summary = JSON.parse(decoder.decode(body));
      } catch (e) {
        out.error = `could not read summary: ${e.message}`;
      }
    } else {
      out.files.push({ name: recordName, kind, bytes: body });
    }
  }

  wasm.pf_free(resultPtr, total);
  return out;
}

/** Trigger a browser download for one generated file. */
export function download(file) {
  const type = file.kind === KIND.PDF ? 'application/pdf' : 'application/xml';
  const url = URL.createObjectURL(new Blob([file.bytes], { type }));
  const a = document.createElement('a');
  a.href = url;
  a.download = file.name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Revoke on the next tick so the click has consumed the URL.
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

/** Object URL for previewing a PDF in an iframe. Caller revokes. */
export function previewUrl(file) {
  return URL.createObjectURL(new Blob([file.bytes], { type: 'application/pdf' }));
}

export { KIND };


/**
 * Re-encode any image the browser can decode into a baseline JPEG.
 *
 * The WASM core embeds JPEG and non-transparent PNG by passing the compressed
 * data straight through, which means it cannot take a PNG with an alpha
 * channel — that would need the pixels decoded. The browser already has a
 * decoder for every format it displays, so flattening onto white here lets a
 * user drop in a transparent PNG, a WebP or an SVG and have it just work.
 *
 * @param {File|Blob} file
 * @param {number} maxWidth  logos are printed small; no point embedding more
 * @returns {Promise<Uint8Array>}
 */
export async function imageToJpeg(file, maxWidth = 600) {
  const url = URL.createObjectURL(file);
  try {
    const img = await new Promise((resolve, reject) => {
      const i = new Image();
      i.onload = () => resolve(i);
      i.onerror = () => reject(new Error('could not read that image'));
      i.src = url;
    });

    const scale = Math.min(1, maxWidth / (img.naturalWidth || maxWidth));
    const w = Math.max(1, Math.round((img.naturalWidth || maxWidth) * scale));
    const h = Math.max(1, Math.round((img.naturalHeight || maxWidth) * scale));

    const canvas = document.createElement('canvas');
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext('2d');
    // Flatten onto white: JPEG has no alpha, and leaving it black would turn
    // every transparent logo into a black box.
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, w, h);
    ctx.drawImage(img, 0, 0, w, h);

    const blob = await new Promise(r => canvas.toBlob(r, 'image/jpeg', 0.92));
    if (!blob) throw new Error('could not convert that image');
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    URL.revokeObjectURL(url);
  }
}
