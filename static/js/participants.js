// Presence: the roster (rendered from the sync module's sse:participants
// frames), the "I'm done writing" toggle, and the docked/overlay panel with
// its resize handle and navigational state.

import { participantId } from './identity.js';

const slug = document.body.dataset.retroSlug;

// The current participant's presence key, used to recognize our own entry
// in the roster. In auth mode the server renders it; in demo mode it is
// derived from the same localStorage id the SSE connection sends.
const selfKey = document.body.dataset.participantKey || ('guest:' + participantId());

// Whether we have marked ourselves as done writing. Kept in sync with the
// roster so a reload (or another tab) reflects the server's state.
let selfReady = false;

function applyReadyState(ready) {
  selfReady = ready;
  const button = document.getElementById('ready-toggle');
  if (!button) return;
  button.classList.toggle('ready', ready);
  button.setAttribute('aria-pressed', ready ? 'true' : 'false');
  button.title = ready
    ? 'You are marked as done writing (click to resume)'
    : 'Let others know you are done writing cards';
  const label = button.querySelector('.ready-toggle-label');
  if (label) label.textContent = ready ? 'Ready ✓' : "I'm done writing";
}

document.body.addEventListener('click', function (event) {
  const button = event.target.closest('#ready-toggle');
  if (!button) return;
  const next = !selfReady;
  // Optimistic: the roster snapshot that follows confirms the state.
  applyReadyState(next);
  const body = new URLSearchParams();
  body.set('ready', next ? 'true' : 'false');
  body.set('participant', participantId());
  fetch('/retro/' + slug + '/ready', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: body.toString(),
  }).then(function (response) {
    if (!response.ok) throw new Error('ready update failed: ' + response.status);
  }).catch(function (error) {
    console.error('failed to update ready state', error);
    applyReadyState(!next);
  });
});

// Render the roster from scratch with DOM APIs (never innerHTML): display
// names are user-supplied.
function renderParticipants(roster) {
  const list = document.getElementById('participants-list');
  const count = document.getElementById('participants-count');
  if (!list) return;
  list.textContent = '';
  roster.forEach(function (participant) {
    const li = document.createElement('li');
    li.className = 'participant';
    if (participant.ready) li.classList.add('participant-ready');
    if (participant.avatar_url) {
      const img = document.createElement('img');
      img.className = 'participant-avatar';
      img.src = participant.avatar_url;
      img.alt = participant.name;
      img.title = participant.name;
      li.appendChild(img);
    } else {
      const span = document.createElement('span');
      span.className = 'participant-initials';
      span.textContent = participant.initials;
      span.title = participant.name;
      li.appendChild(span);
    }
    const name = document.createElement('span');
    name.className = 'participant-name';
    name.textContent = participant.name;
    li.appendChild(name);
    // Readiness indicator: a check when done writing, a pencil otherwise.
    const badge = document.createElement('span');
    badge.className = 'participant-ready-badge';
    badge.textContent = participant.ready ? '✓' : '✎';
    badge.title = participant.ready ? 'Done writing' : 'Still writing';
    li.appendChild(badge);
    list.appendChild(li);
    if (participant.key === selfKey) {
      applyReadyState(participant.ready);
    }
  });
  if (count) count.textContent = String(roster.length);
}

document.body.addEventListener('sse:participants', function (event) {
  renderParticipants(event.detail.participants);
});

// Participants panel: open/close, drag-to-resize, and the reserved page
// width. The panel is docked on desktop and an overlay on narrow screens.
(function () {
  const panel = document.getElementById('participants-panel');
  const toggle = document.getElementById('participants-toggle');
  const closeButton = document.getElementById('participants-close');
  const handle = document.getElementById('participants-resize-handle');
  if (!panel || !toggle) return;

  const DESKTOP = window.matchMedia('(min-width: 900px)');
  const MIN_WIDTH = 160;
  const MAX_WIDTH = 480;

  // The panel's open/closed state is part of the browser's navigational
  // state: it is carried in the `participants` query parameter, so the
  // board is deep-linkable and Back/Forward toggles the panel.
  const PARTICIPANTS_PARAM = 'participants';

  function isOpen() {
    return document.body.classList.contains('participants-open');
  }

  // The state recorded in the URL, or null when the URL carries no explicit
  // preference (then the responsive default applies).
  function stateFromUrl() {
    const value = new URLSearchParams(window.location.search).get(PARTICIPANTS_PARAM);
    if (value === 'open') return true;
    if (value === 'closed') return false;
    return null;
  }

  function applyOpen(open) {
    document.body.classList.toggle('participants-open', open);
    document.body.classList.toggle('participants-collapsed', !open);
    toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
  }

  // User-initiated change: apply it and push a history entry so Back and
  // Forward navigate between panel states.
  function setOpen(open) {
    applyOpen(open);
    if (stateFromUrl() !== open) {
      const url = new URL(window.location.href);
      url.searchParams.set(PARTICIPANTS_PARAM, open ? 'open' : 'closed');
      history.pushState({ participants: open }, '', url);
    }
  }

  // Keep the reserved page width in sync with the panel's actual width
  // (it starts at its content width and changes as the roster or the
  // drag handle changes it).
  if (window.ResizeObserver) {
    const observer = new ResizeObserver(function () {
      document.body.style.setProperty('--participants-width', panel.offsetWidth + 'px');
      if (handle) handle.setAttribute('aria-valuenow', String(panel.offsetWidth));
    });
    observer.observe(panel);
  }

  toggle.addEventListener('click', function () {
    setOpen(true);
  });
  if (closeButton) {
    closeButton.addEventListener('click', function () {
      setOpen(false);
    });
  }

  // Open on desktop, collapsed on narrow screens, unless the URL carries an
  // explicit preference (a shared link or a Back/Forward navigation).
  const initial = stateFromUrl();
  applyOpen(initial === null ? DESKTOP.matches : initial);

  // Back/Forward restores the panel state recorded in the URL.
  window.addEventListener('popstate', function () {
    const state = stateFromUrl();
    applyOpen(state === null ? DESKTOP.matches : state);
  });

  // Clicking the backdrop of the mobile overlay closes the panel.
  document.addEventListener('click', function (event) {
    if (!isOpen() || DESKTOP.matches) return;
    if (panel.contains(event.target) || toggle.contains(event.target)) return;
    setOpen(false);
  });

  document.addEventListener('keydown', function (event) {
    if (event.key === 'Escape' && isOpen() && !DESKTOP.matches) setOpen(false);
  });

  if (!handle) return;

  function clampWidth(width) {
    const max = Math.min(MAX_WIDTH, Math.round(window.innerWidth * 0.8));
    return Math.max(MIN_WIDTH, Math.min(max, width));
  }

  function applyWidth(width) {
    panel.style.width = clampWidth(width) + 'px';
  }

  let startX = 0;
  let startWidth = 0;
  let dragging = false;

  handle.addEventListener('pointerdown', function (event) {
    dragging = true;
    startX = event.clientX;
    startWidth = panel.offsetWidth;
    try {
      handle.setPointerCapture(event.pointerId);
    } catch {
      // Capture is best-effort; dragging still works while over the handle.
    }
    event.preventDefault();
  });

  handle.addEventListener('pointermove', function (event) {
    if (!dragging) return;
    // Dragging left widens the panel.
    applyWidth(startWidth + (startX - event.clientX));
  });

  function stopDragging(event) {
    dragging = false;
    if (handle.hasPointerCapture(event.pointerId)) {
      handle.releasePointerCapture(event.pointerId);
    }
  }

  handle.addEventListener('pointerup', stopDragging);
  handle.addEventListener('pointercancel', stopDragging);

  // Keyboard resizing for the focusable separator.
  handle.addEventListener('keydown', function (event) {
    if (event.key === 'ArrowLeft') {
      applyWidth(panel.offsetWidth + 16);
      event.preventDefault();
    } else if (event.key === 'ArrowRight') {
      applyWidth(panel.offsetWidth - 16);
      event.preventDefault();
    }
  });
})();
