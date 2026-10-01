// Stable per-browser identity for demo-mode presence. localStorage (not
// sessionStorage) keeps it across refreshes, EventSource reconnects, and
// multiple tabs, so one browser counts as one participant. The server ignores
// the parameter in auth mode (the session identity wins).

export function participantId() {
  const key = 'rostfacto_participant_id';
  try {
    let id = window.localStorage.getItem(key);
    if (!id) {
      id = (window.crypto && window.crypto.randomUUID)
        ? window.crypto.randomUUID()
        : String(Date.now()) + '-' + Math.random().toString(16).slice(2);
      window.localStorage.setItem(key, id);
    }
    return id;
  } catch {
    // localStorage unavailable (private mode, disabled): fall back to an
    // in-memory id for this page load.
    return String(Date.now()) + '-' + Math.random().toString(16).slice(2);
  }
}
