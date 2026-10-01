// Server-authoritative countdown: the deadline comes from the DB
// (timer_ends_at, rendered as data-end-at), never from a local map, so
// every client shows the same countdown. The sync module forwards the
// TIMER_* SSE events as sse:timer-* CustomEvents on <body>.

const timerIntervals = new Map(); // badge element -> interval id
// The most recent authoritative deadline per item, from TIMER_STARTED /
// TIMER_EXTENDED payloads. Card re-fetches can be stale (the owner's
// auto-start POST lands after the highlight response, and SSE re-fetches
// race it), so renderTimer falls back to this when a badge has no
// deadline of its own.
const timerDeadlines = new Map(); // item id -> ends_at (epoch ms)

function formatTime(totalSeconds) {
  const seconds = Math.max(0, Math.ceil(totalSeconds));
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return m + ':' + String(s).padStart(2, '0');
}

function stopInterval(badge) {
  const interval = timerIntervals.get(badge);
  if (interval) {
    clearInterval(interval);
    timerIntervals.delete(badge);
  }
}

function updateBadge(badge, remainingMs) {
  const wrap = badge.closest('.timer-wrap');
  const extendBtn = wrap ? wrap.querySelector('.timer-extend') : null;

  if (remainingMs <= 0) {
    badge.textContent = '0:00';
    badge.classList.remove('timer-warning');
    badge.classList.add('timer-over');
    if (extendBtn) extendBtn.hidden = false;
  } else {
    badge.textContent = formatTime(remainingMs / 1000);
    badge.classList.remove('timer-over');
    if (extendBtn) extendBtn.hidden = true;
    if (remainingMs <= 30000) {
      badge.classList.add('timer-warning');
    } else {
      badge.classList.remove('timer-warning');
    }
  }
}

function setCountdown(badge, endAtMs) {
  stopInterval(badge);
  badge.dataset.endAt = String(endAtMs);
  const tick = function () {
    if (!badge.isConnected) {
      stopInterval(badge);
      return;
    }
    const remaining = endAtMs - Date.now();
    updateBadge(badge, remaining);
    if (remaining <= 0) stopInterval(badge);
  };
  tick();
  if (endAtMs - Date.now() > 0) {
    timerIntervals.set(badge, setInterval(tick, 1000));
  }
}

// Render one badge from its server-rendered state.
function renderTimer(badge) {
  if (badge.hasAttribute('data-elapsed')) {
    badge.textContent = '0:00';
    badge.classList.remove('timer-warning');
    badge.classList.add('timer-over');
    const extendBtn = badge.closest('.timer-wrap')
      ? badge.closest('.timer-wrap').querySelector('.timer-extend')
      : null;
    if (extendBtn) extendBtn.hidden = false;
    stopInterval(badge);
    return;
  }
  const endAt = parseInt(badge.dataset.endAt, 10);
  if (!isNaN(endAt)) {
    setCountdown(badge, endAt);
  } else {
    // The badge may come from a stale render (a card fetched before the
    // timer auto-start landed). The deadline is server-authoritative, so
    // fall back to the one carried by the timer events.
    const card = badge.closest('article.card');
    const itemId = card ? card.dataset.itemId : null;
    const knownEndAt = itemId ? timerDeadlines.get(itemId) : undefined;
    if (typeof knownEndAt === 'number') {
      setCountdown(badge, knownEndAt);
      return;
    }
    // Highlighted but not started yet: show the initial duration statically.
    badge.textContent = formatTime(parseInt(badge.dataset.initialSeconds || '300', 10));
    badge.classList.remove('timer-over');
    badge.classList.remove('timer-warning');
    const extendBtn = badge.closest('.timer-wrap')
      ? badge.closest('.timer-wrap').querySelector('.timer-extend')
      : null;
    if (extendBtn) extendBtn.hidden = true;
    stopInterval(badge);
  }
}

function renderAllTimers() {
  document.querySelectorAll('.timer-badge').forEach(renderTimer);
}

// Timer responses update the badge in place: replacing the whole card
// would detach it and break in-flight HTMX swaps (e.g. a quick "Done"
// click after highlighting).
function applyTimerResponseHtml(itemId, html) {
  const template = document.createElement('template');
  template.innerHTML = html.trim();
  const newBadge = template.content.querySelector('.timer-badge');
  const newExtend = template.content.querySelector('.timer-extend');
  const current = document.querySelector('article.card[data-item-id="' + itemId + '"]');
  if (!current || !newBadge) return;
  const oldBadge = current.querySelector('.timer-badge');
  if (!oldBadge) return;
  const wrap = oldBadge.closest('.timer-wrap');
  if (wrap) {
    const oldExtend = wrap.querySelector('.timer-extend');
    oldBadge.replaceWith(newBadge);
    if (oldExtend && newExtend) oldExtend.replaceWith(newExtend);
  } else {
    oldBadge.replaceWith(newBadge);
  }
  renderTimer(newBadge);
}

function postTimerAction(itemId, path, params) {
  fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: params ? params.toString() : '',
  }).then(function (response) {
    const eventId = response.headers.get('X-Event-Id');
    if (!response.ok) throw new Error('timer request failed: ' + response.status);
    if (eventId) {
      // The response already applied the change; ignore the matching SSE
      // event. Only dedup successful responses: an error response cannot
      // have applied the change, and dropping the SSE event would lose
      // the deadline entirely.
      document.body.dispatchEvent(
        new CustomEvent('sse:event-applied', { detail: { id: eventId } }),
      );
    }
    return response.text();
  }).then(function (html) {
    applyTimerResponseHtml(itemId, html);
  }).catch(function (error) {
    console.error('SSE: timer request failed', itemId, error);
  }).finally(function () {
    pendingTimerPosts.delete(String(itemId));
  });
}

function startTimerRequest(itemId, durationSeconds) {
  pendingTimerPosts.set(String(itemId), true);
  const params = new URLSearchParams();
  params.set('duration', String(durationSeconds));
  postTimerAction(itemId, '/items/' + itemId + '/timer/start', params);
}

function extendTimerRequest(itemId) {
  pendingTimerPosts.set(String(itemId), true);
  postTimerAction(itemId, '/items/' + itemId + '/timer/extend', null);
}

// A timer is started by the client that highlighted the card, so only
// htmx-driven swaps (our own actions) auto-start; SSE swaps never do.
// afterRequest fires after the swap (and again on a parent because the
// request element is detached), so look up the highlighted card in the DOM
// rather than using the request element.
document.body.addEventListener('htmx:afterRequest', function (event) {
  const detail = event.detail;
  if (!detail || !detail.successful) return;
  const path = (detail.pathInfo && detail.pathInfo.requestPath) || '';
  if (path.indexOf('action=highlight') === -1) return;
  const elt = (detail.requestConfig && detail.requestConfig.elt) || detail.elt;
  const itemId = elt && elt.dataset ? elt.dataset.itemId : null;
  if (!itemId) return;
  const card = document.querySelector('article.card.highlighted[data-item-id="' + itemId + '"]');
  if (!card) return; // e.g. the single-highlight conflict re-rendered the created card
  const badge = card.querySelector('.timer-badge');
  if (!badge || badge.hasAttribute('data-end-at') || badge.hasAttribute('data-elapsed')) return;
  const duration = parseInt(document.body.dataset.timerDefaultSeconds || '300', 10);
  startTimerRequest(itemId, duration);
});

// Another client's timer event: remember the authoritative deadline and
// render the same countdown from it. Keys are strings so they match
// card.dataset.itemId lookups in renderTimer.
document.body.addEventListener('sse:timer-updated', function (event) {
  timerDeadlines.set(String(event.detail.itemId), event.detail.endsAt);
  const card = document.querySelector('article.card[data-item-id="' + event.detail.itemId + '"]');
  if (!card) return;
  const badge = card.querySelector('.timer-badge');
  if (badge) setCountdown(badge, event.detail.endsAt);
});

// A status change ends the previous timer cycle (completing or cancelling
// resets the timer columns); forget its deadline so a stale render of the
// next highlight cannot pick it up.
document.body.addEventListener('sse:timer-reset', function (event) {
  timerDeadlines.delete(String(event.detail.itemId));
});

// The owner's own status-change events are deduplicated, so the highlight
// request itself is the reliable cycle boundary on this client: a deadline
// left over from the previous cycle must not block the auto-start below.
// The same hook arms the auto-start fallback (see below).
document.body.addEventListener('htmx:beforeRequest', function (event) {
  const elt = (event.detail &&
    (event.detail.requestConfig && event.detail.requestConfig.elt || event.detail.elt)) || null;
  const itemId = elt && elt.dataset ? elt.dataset.itemId : null;
  if (itemId) timerDeadlines.delete(itemId);
  const path = (event.detail && event.detail.requestConfig && event.detail.requestConfig.path) ||
    '';
  if (path.indexOf('action=highlight') === -1) return;
  if (itemId) armHighlightFallback(String(itemId));
});

// The auto-start normally fires from htmx:afterRequest, but htmx 2.0.10
// drops that event when the SSE re-fetch for the same status change
// replaces the request element before the highlight response lands: the
// surviving-ancestor re-trigger walks the element's (now unreachable)
// ancestors, finds none, and never dispatches. The timer would then never
// start, so this fallback arms itself when the highlight request goes out
// (htmx:beforeRequest always fires) and starts the timer if the badge
// still has no deadline. The deadline is server-authoritative and the
// start is guarded by pendingTimerPosts, so a racing fast-path start
// cannot double-start the timer.
const pendingTimerPosts = new Map(); // item id (string) -> true while a POST is in flight
const highlightFallbacks = new Map(); // item id (string) -> interval id
function armHighlightFallback(key) {
  if (highlightFallbacks.has(key)) return;
  let attempts = 0;
  const interval = setInterval(function () {
    attempts++;
    const card = document.querySelector('article.card[data-item-id="' + key + '"]');
    if (!card) {
      clearInterval(interval);
      highlightFallbacks.delete(key);
      return;
    }
    if (!card.classList.contains('highlighted')) {
      // The highlight did not stick (e.g. the single-highlight conflict).
      clearInterval(interval);
      highlightFallbacks.delete(key);
      return;
    }
    const badge = card.querySelector('.timer-badge');
    if (!badge || badge.hasAttribute('data-end-at') || badge.hasAttribute('data-elapsed')) {
      // The deadline landed through the fast path or an SSE event.
      clearInterval(interval);
      highlightFallbacks.delete(key);
      return;
    }
    if (timerDeadlines.has(key) || pendingTimerPosts.has(key)) return; // already handled or in flight
    if (attempts >= 10) {
      clearInterval(interval);
      highlightFallbacks.delete(key);
      return;
    }
    const duration = parseInt(document.body.dataset.timerDefaultSeconds || '300', 10);
    startTimerRequest(key, duration);
  }, 1500);
  highlightFallbacks.set(key, interval);
}

document.body.addEventListener('sse:timer-elapsed', function (event) {
  const card = document.querySelector('article.card[data-item-id="' + event.detail.itemId + '"]');
  if (!card) return;
  const badge = card.querySelector('.timer-badge');
  if (badge) {
    badge.textContent = '0:00';
    badge.classList.remove('timer-warning');
    badge.classList.add('timer-over');
    // Keep the server-rendered deadline on the badge: it is the
    // authoritative record of when the timer ended, and a stale card
    // re-fetch may render an elapsed badge without a deadline of its own.
    const extendBtn = card.querySelector('.timer-extend');
    if (extendBtn) extendBtn.hidden = false;
    stopInterval(badge);
  }
});

document.addEventListener('DOMContentLoaded', renderAllTimers);
document.body.addEventListener('htmx:afterSettle', renderAllTimers);
document.body.addEventListener('sse:card-swapped', renderAllTimers);
document.body.addEventListener('click', function (e) {
  const button = e.target.closest('.timer-extend');
  if (button) {
    e.stopPropagation();
    const card = button.closest('article.card');
    if (card && card.dataset.itemId) extendTimerRequest(card.dataset.itemId);
  }
});
