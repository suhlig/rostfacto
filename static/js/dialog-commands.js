// Emulates the Invoker Commands API (`command`/`commandfor`) on browsers that
// don't support it (Firefox < 144, Safari < 26.2). Kept in its own module so it
// can be unit-tested under `deno test` with a stubbed DOM.

// Installs a click listener that opens/closes the target dialog for
// `command="show-modal"`/`command="close"` buttons. No-op when the browser
// supports Invoker Commands natively (the native behaviour handles the click).
export function installDialogCommandFallback(win, doc) {
  if ('command' in win.HTMLButtonElement.prototype) return;
  doc.addEventListener('click', function (event) {
    const button = event.target.closest('[command][commandfor]');
    if (!button) return;
    const dialog = doc.getElementById(button.getAttribute('commandfor'));
    if (!dialog || typeof dialog.showModal !== 'function') return;
    if (button.getAttribute('command') === 'show-modal') {
      if (!dialog.open) dialog.showModal();
    } else if (button.getAttribute('command') === 'close') {
      dialog.close();
    }
  });
}
