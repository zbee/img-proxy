// img-proxy dashboard client script.
// Handles the add/update and delete forms, the copy-to-clipboard buttons, and
// toast notifications. Every mutating action talks to the worker through
// fetch() and swaps in the returned #dashboard-content fragment.

// The dashboard key is embedded in the page by the server and reused for
// every fetch() call the script makes (delete, reload).
const KEY = document.body.dataset.key;

// Copies an image's public URL (with its real extension) to the clipboard.
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

// Copies the just-added image's URL and briefly relabels the copy button.
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
    clearTimeout(window._toastTimer);
    window._toastTimer = setTimeout(() => el.classList.add('opacity-0'), 1500);
}

// Escape key closes the delete-confirmation modal from anywhere on the page.
document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeConfirm();
});

// Tile copy, tile delete, and the success-box copy are delegated here instead
// of inline onclick attributes. Clicking the readonly result URL both selects
// and copies it, matching the tiles' click-to-copy behavior.
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
    if (e.target.closest('#copy-btn') || e.target.closest('#result-url')) copyResult();
});

// Focusing the result URL (click or keyboard) selects its text so it's
// immediately copyable the old-fashioned way too.
document.addEventListener('focusin', (e) => {
    if (e.target.id === 'result-url') e.target.select();
});

// Replaces #dashboard-content with the returned fragment. The View Transition
// API was causing the UI to hang and images to render incorrectly, so this is
// now a plain, synchronous DOM replacement.
function swapContent(html) {
    const current = document.getElementById('dashboard-content');
    const template = document.createElement('template');
    template.innerHTML = html.trim();
    const next = template.content.getElementById('dashboard-content');
    if (current && next) {
        current.replaceWith(next);
    }
}

// Fetches `url` (tagged so the worker returns a fragment, not a full page),
// throws on a non-OK response, and swaps the result in.
async function loadContent(url, opts) {
    const res = await fetch(url, Object.assign({ headers: { 'X-Requested-With': 'fetch' } }, opts));
    if (!res.ok) throw new Error('request failed: ' + res.status);
    swapContent(await res.text());
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

// Toggles the submit button's disabled state and swaps its label while a
// save is in flight.
function setBusy(btn, busy, label) {
    if (!btn) return;
    btn.disabled = busy;
    if (busy) { btn.dataset.label = btn.textContent; btn.textContent = label; }
    else btn.textContent = btn.dataset.label || 'Add / Update';
}

// Handles the add/update form. Every schedule — including "Never" (upload
// once) — is just a POST to the worker: it fetches the source, stores the
// bytes, and hands back a fragment with the new tile already in the gallery.
document.getElementById('add-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const form = e.target;
    const btn = form.querySelector('button[type="submit"]');
    setBusy(btn, true, 'saving…');
    try {
        await loadContent(form.action, { method: form.method, body: new FormData(form) });
    } catch (err) {
        toast(err.message || 'save failed');
    } finally {
        setBusy(btn, false);
    }
});
