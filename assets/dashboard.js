// img-proxy dashboard client script.
// Handles the add/update and delete forms, the copy-to-clipboard buttons,
// toast notifications, and (for "Never" uploads) in-browser image/video
// conversion via ffmpeg.wasm. Every mutating action talks to the worker
// through fetch() and swaps in the returned #dashboard-content fragment,
// wrapping the swap in a View Transition when the browser supports one.

// The dashboard key is embedded in the page by the server and reused for
// every fetch() call the script makes (delete, fetch-proxy, upload, reload).
const KEY = document.body.dataset.key;

// Copies an image's public URL to the clipboard and toasts success/failure.
function copyImage(name, ext) {
    const url = window.location.origin + '/' + name + (ext || '');
    navigator.clipboard.writeText(url).then(
        () => toast('copied ' + name),
        () => toast('copy failed')
    );
}

// Fills in and opens the "delete this image?" confirmation modal.
function openConfirm(name) {
    document.getElementById('confirm-name').textContent = name;
    document.getElementById('confirm-img').src = '/' + name;
    document.getElementById('confirm-form').action = '/delete/' + name + '?key=' + KEY;
    document.getElementById('modal').classList.remove('hidden');
}

// Hides the delete-confirmation modal.
function closeConfirm() {
    document.getElementById('modal').classList.add('hidden');
}

// Copies the just-uploaded image's URL and briefly relabels the copy button.
function copyResult() {
    const el = document.getElementById('result-url');
    if (!el) return;
    navigator.clipboard.writeText(el.value.trim()).then(() => {
        const btn = document.getElementById('copy-btn');
        if (!btn) return;
        const label = btn.textContent;
        btn.textContent = 'copied!';
        setTimeout(() => { btn.textContent = label; }, 1500);
    }).catch(() => {});
}

// Shows a transient bottom-of-screen message, creating the toast element on
// first use and reusing it afterwards.
function toast(msg) {
    let el = document.getElementById('toast');
    if (!el) {
        el = document.createElement('div');
        el.id = 'toast';
        el.className = 'fixed bottom-4 left-1/2 -translate-x-1/2 bg-mocha-mantle border border-mocha-surface0 text-mocha-text text-xs px-4 py-2 rounded-lg shadow-xl shadow-black/50 z-50 transition-opacity duration-200';
        document.body.appendChild(el);
    }
    el.textContent = msg;
    el.classList.remove('opacity-0');
    // _toastTimer is stashed on `window` (rather than a local variable)
    // because toast() has no persistent scope of its own between calls;
    // clearing the previous timer first stops an old toast from fading out
    // a new message that arrived before the old one's timeout fired.
    clearTimeout(window._toastTimer);
    window._toastTimer = setTimeout(() => el.classList.add('opacity-0'), 1500);
}

// Escape key closes the delete-confirmation modal from anywhere on the page.
document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeConfirm();
});

// Tile copy, tile delete, and the success-box copy button are delegated here
// instead of inline onclick attributes.
document.addEventListener('click', (e) => {
    if (e.target.closest('button[data-name]')) {
        openConfirm(e.target.closest('button[data-name]').dataset.name);
        return;
    }
    const tile = e.target.closest('.tile');
    if (tile) {
        copyImage(tile.dataset.name, tile.dataset.ext);
        return;
    }
    if (e.target.closest('#copy-btn')) copyResult();
});

//region Seamless Transitions
// Replaces the #dashboard-content element with the markup in `html`,
// animated via the View Transition API when the browser supports it.
function swapContent(html) {
    const doSwap = async () => {
        const current = document.getElementById('dashboard-content');
        // A <template> is used purely as an inert parser: it lets us turn
        // the fragment string into DOM nodes without inserting them into
        // the live page first, so we can pull out just the new
        // #dashboard-content node below.
        const template = document.createElement('template');
        template.innerHTML = html.trim();
        const next = template.content.getElementById('dashboard-content');
        if (current && next) {
            current.replaceWith(next);
            // Ensure any new images in the gallery are decoded before completing
            // the transition so the browser knows their layout size instead of
            // capturing a collapsed 0-height sliver snapshot.
            const imgs = Array.from(next.querySelectorAll('img'));
            await Promise.all(imgs.map(img => {
                if (img.complete) return Promise.resolve();
                if (img.decode) return img.decode().catch(() => {});
                return new Promise(resolve => {
                    img.onload = resolve;
                    img.onerror = resolve;
                });
            }));
        }
    };
    if (document.startViewTransition) {
        return document.startViewTransition(doSwap);
    } else {
        return doSwap();
    }
}

// Fetches `url` (tagged so the worker knows to return a fragment, not a
// full page), throws on a non-OK response, and swaps the result in.
async function loadContent(url, opts) {
    const res = await fetch(url, Object.assign({ headers: { 'X-Requested-With': 'fetch' } }, opts));
    if (!res.ok) throw new Error('request failed: ' + res.status);
    await swapContent(await res.text());
    return res;
}

// Submits the delete-confirmation form via fetch instead of a full
// navigation, then closes the modal regardless of outcome.
document.getElementById('confirm-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const form = e.target;
    try {
        await loadContent(form.action, { method: form.method });
    } catch (err) {
        toast(err.message || 'delete failed');
    } finally {
        closeConfirm();
    }
});
//endregion

//region Client-Side Conversion/Compression
// ffmpeg.wasm is large, so it's loaded lazily and cached in this module
// variable rather than fetched on every "Never" upload.
let ffmpeg = null;

// ffmpeg.wasm's load() can hang *forever* if its worker fails to start —
// a CORS error is the usual culprit — so every await below
// is timed against this to fail properly.
const FFMPEG_LOAD_TIMEOUT_MS = 20_000;

// Race against the timeout to fail properly.
function withTimeout(promise, ms, message) {
    return Promise.race([
        promise,
        new Promise((_, reject) => setTimeout(() => reject(new Error(message)), ms)),
    ]);
}

// Lazily loads and initializes ffmpeg.wasm, caching the instance for reuse.
async function ensureFfmpeg() {
    if (ffmpeg) return ffmpeg;

    let instance;
    try {
        const [{ FFmpeg }, { toBlobURL }] = await withTimeout(
            Promise.all([
                import('https://cdn.jsdelivr.net/npm/@ffmpeg/ffmpeg@0.12.15/dist/esm/index.js'),
                import('https://cdn.jsdelivr.net/npm/@ffmpeg/util@0.12.2/dist/esm/index.js'),
            ]),
            FFMPEG_LOAD_TIMEOUT_MS,
            'timed out downloading the converter (CDN unreachable?)'
        );

        instance = new FFmpeg();
        const baseURL = 'https://cdn.jsdelivr.net/npm/@ffmpeg/core@0.12.10/dist/esm';

        const [coreURL, wasmURL] = await withTimeout(
            Promise.all([
                toBlobURL(`${baseURL}/ffmpeg-core.js`, 'text/javascript'),
                toBlobURL(`${baseURL}/ffmpeg-core.wasm`, 'application/wasm'),
            ]),
            FFMPEG_LOAD_TIMEOUT_MS,
            'timed out downloading the converter core (CDN unreachable?)'
        );


        await withTimeout(
            instance.load({
                coreURL,
                wasmURL,
                classWorkerURL: window.location.origin + '/ffmpeg/worker.js', // definitely a real file
            }),
            FFMPEG_LOAD_TIMEOUT_MS,
            'timed out starting the converter (same-origin worker failed? check the console)'
        );

        ffmpeg = instance;
        return ffmpeg;
    } catch (err) {
        // A half-loaded instance is poison: drop it so the next attempt
        // starts fresh instead of returning a broken cached one.
        ffmpeg = null;
        throw new Error('converter failed to start: ' +
            (err && err.message ? err.message : err));
    }
}

// Converts an in-memory image/video to WebP/WebM via ffmpeg.wasm; SVGs are
// handled by the caller and never reach this function.
async function convert(inputType, inBytes) {
    const isVideo = inputType.startsWith('video/');
    const isGif = inputType === 'image/gif';
    await ffmpeg.writeFile('input', inBytes);
    if (isVideo) {
        await ffmpeg.exec(['-i', 'input', '-c:v', 'libvpx-vp9', '-an',
            '-b:v', '0', '-crf', '40', 'out.webm']);
    } else {
        // GIFs need '-loop 0' to keep looping forever in the WebP output;
        // static images don't take that flag at all.
        const q = isGif ? ['-loop', '0'] : [];
        await ffmpeg.exec(['-i', 'input', '-c:v', 'libwebp', ...q,
            '-q:v', '80', 'out.webp']);
    }
    const out = isVideo ? 'out.webm' : 'out.webp';
    return { bytes: await ffmpeg.readFile(out),
        type: isVideo ? 'video/webm' : 'image/webp' };
}

// Toggles the submit button's disabled state and swaps its label while a
// long-running action (saving/converting) is in flight.
function setBusy(btn, busy, label) {
    if (!btn) return;
    btn.disabled = busy;
    // The original label is stashed on the button's dataset so it can be
    // restored exactly once busy work finishes, even across re-entrant calls.
    if (busy) { btn.dataset.label = btn.textContent; btn.textContent = label; }
    else btn.textContent = btn.dataset.label || 'Add / Update';
}

// Handles the add/update form: regular refresh schedules are posted
// straight to the worker, while "Never" (upload-once) sources are
// converted client-side first and uploaded as a ready-to-serve blob.
document.getElementById('add-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const form = e.target;
    const btn = form.querySelector('button[type="submit"]');

    if (form.elements.frequency.value !== '0') {
        // Regular refresh schedules: the worker fetches & stores the
        // image itself, we just post the form and swap in the result.
        setBusy(btn, true, 'saving…');
        try {
            await loadContent(form.action, { method: form.method, body: new FormData(form) });
        } catch (err) {
            toast(err.message || 'save failed');
        } finally {
            setBusy(btn, false);
        }
        return;
    }

    // "Never" uploads are converted client-side (ffmpeg.wasm) before
    // being handed to the worker as a ready-to-serve blob.
    const source = form.elements.source.value.trim();
    const name = form.elements.name.value.trim();
    setBusy(btn, true, 'converting…');

    try {
        // The worker proxies the fetch so the browser doesn't hit CORS
        // issues pulling an arbitrary third-party source URL directly.
        const proxy = await fetch(`/fetch?key=${KEY}&url=${encodeURIComponent(source)}`);
        if (!proxy.ok) throw new Error('could not fetch source: ' + proxy.status);

        const inputType = proxy.headers.get('content-type') || '';
        const inBytes = new Uint8Array(await proxy.arrayBuffer());

        let outBytes, outType;
        if (inputType.startsWith('image/svg')) {
            // SVGs pass through untouched; ffmpeg has nothing useful to do here.
            outBytes = inBytes;
            outType = inputType;
        } else {
            await ensureFfmpeg();
            const out = await convert(inputType, inBytes);
            outBytes = out.bytes;
            outType = out.type;
        }

        if (outBytes.byteLength > 25 * 1024 * 1024) {
            throw new Error('result is ' + (outBytes.byteLength / 1048576).toFixed(1) +
                ' MiB, over 25 MiB');
        }

        const up = await fetch(
            `/upload?key=${KEY}&name=${encodeURIComponent(name)}` +
            `&source=${encodeURIComponent(source)}&content_type=${encodeURIComponent(outType)}`,
            { method: 'POST', body: outBytes }
        );
        if (!up.ok) throw new Error('upload failed: ' + up.status);

        await loadContent('/?key=' + KEY + '&added=' + encodeURIComponent(name));
    } catch (err) {
        toast(err.message || 'conversion failed');
    } finally {
        setBusy(btn, false);
    }
});
//endregion
