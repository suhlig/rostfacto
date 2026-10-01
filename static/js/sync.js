// Cross-client sync: apply events from the SSE stream, deduplicating the
// events our own mutations produce (those are also applied via HTMX). This
// module owns the single EventSource for the board; other modules react to the
// CustomEvents it dispatches on <body> (sse:participants, sse:card-swapped,
// sse:timer-*, sse:action-item-added) rather than opening their own connection.

import { participantId } from './identity.js';

const slug = document.body.dataset.retroSlug;
const appliedEventIds = new Set();
const MAX_APPLIED_IDS = 200;

// Remember the ids of events our own mutations produced, so the matching
// SSE events are not applied twice. htmx 4 uses fetch(), so response headers
// live on the request context rather than on an XHR object.
document.body.addEventListener('htmx:after:request', function (event) {
  const ctx = event.detail && event.detail.ctx;
  const headers = ctx && ctx.response && ctx.response.headers;
  if (!headers) return;
  const eventId = headers.get('X-Event-Id');
  if (!eventId) return;
  appliedEventIds.add(eventId);
  while (appliedEventIds.size > MAX_APPLIED_IDS) {
    appliedEventIds.delete(appliedEventIds.values().next().value);
  }
});
// Plain fetch responses (timer start/extend) carry the header too.
document.body.addEventListener('sse:event-applied', function (event) {
  appliedEventIds.add(event.detail.id);
  while (appliedEventIds.size > MAX_APPLIED_IDS) {
    appliedEventIds.delete(appliedEventIds.values().next().value);
  }
});

// The HTMX swap and the SSE delivery of the same mutation can arrive in
// either order, so keep the board free of duplicate cards. When both
// renders of a card are present, keep the one that was rendered last: the
// SSE re-fetch runs after the add response was rendered, so a late-arriving
// add-card response would otherwise replace a card the user just
// highlighted with its pre-highlight render.
function removeDuplicateCards() {
  document.querySelectorAll('.item-list').forEach(function (list) {
    const seen = new Set();
    const latest = new Map();
    list.querySelectorAll('article.card').forEach(function (card) {
      const id = card.dataset.itemId;
      if (seen.has(id)) {
        const older = latest.get(id);
        if (older && older !== card) {
          older.remove();
        }
      }
      latest.set(id, card);
      seen.add(id);
    });
  });
}
document.body.addEventListener('htmx:after:swap', removeDuplicateCards);

function cardExists(itemId) {
  return document.querySelector('article.card[data-item-id="' + itemId + '"]') !== null;
}

function fetchCardHtml(itemId, onSuccess) {
  fetch('/items/' + itemId, { headers: { Accept: 'text/html' } })
    .then(function (response) {
      if (!response.ok) throw new Error('card fetch failed: ' + response.status);
      return response.text();
    })
    .then(onSuccess)
    .catch(function (error) {
      console.error('SSE: failed to fetch card', itemId, error);
    });
}

function notifyCardSwapped() {
  document.body.dispatchEvent(new CustomEvent('sse:card-swapped'));
}

// htmx binds trigger handlers directly on elements when it processes them,
// so cards inserted via SSE (not via an HTMX swap) must be handed to htmx
// explicitly or their buttons stay inert.
function processWithHtmx(elt) {
  if (window.htmx && window.htmx.process) {
    window.htmx.process(elt);
  }
}

function replaceCard(itemId, html) {
  const current = document.querySelector('article.card[data-item-id="' + itemId + '"]');
  const template = document.createElement('template');
  template.innerHTML = html.trim();
  const replacement = template.content.firstElementChild;
  if (current && replacement) {
    // A re-fetch for an older state can land after a newer render (e.g.
    // the highlight re-fetch completing after the card was completed):
    // events are broadcast in commit order, but each re-fetch renders
    // whatever the DB held when its GET ran, so responses can arrive out
    // of order. `data-updated-at` is the item's `updated_at` (bumped by a
    // BEFORE UPDATE trigger on every mutation), so skip a replacement that
    // is strictly older than the card already in the DOM. Equal timestamps
    // still replace: idempotent re-renders and the single-highlight error
    // render must apply.
    const currentUpdatedAt = parseInt(current.dataset.updatedAt, 10);
    const incomingUpdatedAt = parseInt(replacement.dataset.updatedAt, 10);
    if (
      !isNaN(currentUpdatedAt) && !isNaN(incomingUpdatedAt) &&
      incomingUpdatedAt < currentUpdatedAt
    ) {
      return;
    }
    // If the timestamps are equal, the re-fetch can still be stale: when
    // the SSE event for a status change beats the htmx response, the fetch
    // may complete before the owner's timer auto-start lands, and replacing
    // the card would clobber the running countdown. The deadline is
    // server-authoritative, so carry it over to the incoming badge when it
    // is missing.
    const oldBadge = current.querySelector('.timer-badge');
    const newBadge = replacement.querySelector('.timer-badge');
    if (
      oldBadge && newBadge &&
      oldBadge.hasAttribute('data-end-at') &&
      !newBadge.hasAttribute('data-end-at') &&
      !newBadge.hasAttribute('data-elapsed')
    ) {
      newBadge.setAttribute('data-end-at', oldBadge.getAttribute('data-end-at'));
    }
    current.replaceWith(replacement);
    processWithHtmx(replacement);
    notifyCardSwapped();
  }
}

function insertCard(containerId, html) {
  const template = document.createElement('template');
  template.innerHTML = html.trim();
  const card = template.content.firstElementChild;
  if (!card || cardExists(card.dataset.itemId)) return;
  const container = document.getElementById(containerId);
  if (!container) return;
  container.insertBefore(card, container.firstChild);
  removeDuplicateCards();
  processWithHtmx(card);
  notifyCardSwapped();
}

function updateLikeCount(itemId, count) {
  const card = document.querySelector('article.card[data-item-id="' + itemId + '"]');
  if (!card) return;
  const badge = card.querySelector('.like-count');
  if (badge) badge.textContent = String(count);
}

function parseEvent(event) {
  try {
    return JSON.parse(event.data);
  } catch (error) {
    console.error('SSE: malformed event data', event.data, error);
    return null;
  }
}

const source = new EventSource(
  '/retro/' + slug + '/events?participant=' + encodeURIComponent(participantId()),
);

// Presence roster: full snapshots, ephemeral (no `id:` line), so there is
// nothing to deduplicate against the applied-event set. Hand the roster to the
// participants module, which renders it (never innerHTML: display names are
// user-supplied).
source.addEventListener('PARTICIPANTS', function (event) {
  const data = parseEvent(event);
  if (!data || !Array.isArray(data.participants)) return;
  document.body.dispatchEvent(
    new CustomEvent('sse:participants', { detail: { participants: data.participants } }),
  );
});

source.addEventListener('ITEM_CREATED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  fetchCardHtml(data.item_id, function (html) {
    insertCard(data.category.toLowerCase() + '-items', html);
  });
});

source.addEventListener('ITEM_STATUS_CHANGED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  // A status change ends the previous timer cycle (completing or
  // cancelling resets the timer columns); the timer module forgets the
  // deadline so a stale render of the next highlight cannot pick it up.
  document.body.dispatchEvent(
    new CustomEvent('sse:timer-reset', {
      detail: { itemId: data.item_id },
    }),
  );
  fetchCardHtml(data.item_id, function (html) {
    replaceCard(data.item_id, html);
    // Completing the last active card shows the all-done archive modal on
    // every client, not just the one that completed it.
    maybeShowArchiveModal();
  });
});

// The all-done archive modal appears for everyone once no active cards
// remain (mirrors the server's all_completed check).
function maybeShowArchiveModal() {
  const cards = document.querySelectorAll('.item-list article.card');
  if (cards.length === 0) return;
  const hasActive = Array.from(cards).some(function (card) {
    return !card.classList.contains('completed');
  });
  if (hasActive) return;
  const dialog = document.getElementById('archive-modal');
  if (dialog && !dialog.open && typeof dialog.showModal === 'function') {
    dialog.showModal();
  }
}

source.addEventListener('ITEM_UPDATED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  const card = document.querySelector('article.card[data-item-id="' + data.item_id + '"]');
  if (!card) return;
  const text = card.querySelector('.card-text');
  if (text) text.textContent = data.text;
});

source.addEventListener('ITEM_LIKED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  updateLikeCount(data.item_id, data.likes_count);
});

source.addEventListener('ITEM_UNLIKED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  updateLikeCount(data.item_id, data.likes_count);
});

// Action items sync like cards, but they live in their own pool/columns.
function fetchActionItemHtml(actionItemId, onSuccess) {
  fetch('/action-items/' + actionItemId, { headers: { Accept: 'text/html' } })
    .then(function (response) {
      if (!response.ok) throw new Error('action item fetch failed: ' + response.status);
      return response.text();
    })
    .then(onSuccess)
    .catch(function (error) {
      console.error('SSE: failed to fetch action item', actionItemId, error);
    });
}

function actionItemExists(actionItemId) {
  return document.querySelector('.action-item[data-action-item-id="' + actionItemId + '"]') !==
    null;
}

function insertActionItem(html) {
  const template = document.createElement('template');
  template.innerHTML = html.trim();
  const item = template.content.firstElementChild;
  if (!item || actionItemExists(item.dataset.actionItemId)) return;
  const pool = document.getElementById('action-items-pool');
  if (!pool) return;
  pool.insertBefore(item, pool.firstChild);
  processWithHtmx(item);
  // The action-items module re-groups the pool into the dated columns.
  document.body.dispatchEvent(new CustomEvent('sse:action-item-added'));
}

function replaceActionItem(actionItemId, html) {
  const current = document.querySelector(
    '.action-item[data-action-item-id="' + actionItemId + '"]',
  );
  if (!current) return;
  const template = document.createElement('template');
  template.innerHTML = html.trim();
  const replacement = template.content.firstElementChild;
  if (!replacement) return;
  current.replaceWith(replacement);
  processWithHtmx(replacement);
}

source.addEventListener('ACTION_ITEM_CREATED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  fetchActionItemHtml(data.action_item_id, insertActionItem);
});

source.addEventListener('ACTION_ITEM_UPDATED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  const item = document.querySelector(
    '.action-item[data-action-item-id="' + data.action_item_id + '"]',
  );
  if (!item) return;
  const text = item.querySelector('.action-item-text');
  if (text) text.textContent = data.text;
});

source.addEventListener('ACTION_ITEM_COMPLETED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  fetchActionItemHtml(data.action_item_id, function (html) {
    replaceActionItem(data.action_item_id, html);
  });
});

source.addEventListener('ACTION_ITEM_DELETED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  const item = document.querySelector(
    '.action-item[data-action-item-id="' + data.action_item_id + '"]',
  );
  if (item) item.remove();
});

// Timer events carry the authoritative deadline; the timer module renders
// the countdown from it.
source.addEventListener('TIMER_STARTED', handleTimerPayload);
source.addEventListener('TIMER_EXTENDED', handleTimerPayload);
function handleTimerPayload(event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  const endsAt = Date.parse(data.ends_at);
  if (isNaN(endsAt)) return;
  document.body.dispatchEvent(
    new CustomEvent('sse:timer-updated', {
      detail: { itemId: data.item_id, endsAt: endsAt },
    }),
  );
}

source.addEventListener('TIMER_ELAPSED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  const data = parseEvent(event);
  if (!data) return;
  document.body.dispatchEvent(
    new CustomEvent('sse:timer-elapsed', {
      detail: { itemId: data.item_id },
    }),
  );
});

// The retro was archived: clear the board and stop all timers (removing
// the badges stops their countdowns). Archiving also archives action items,
// so clear those too.
source.addEventListener('RETRO_ARCHIVED', function (event) {
  if (appliedEventIds.has(event.lastEventId)) return;
  document.querySelectorAll('.item-list article.card').forEach(function (card) {
    card.remove();
  });
  document.querySelectorAll('.action-item').forEach(function (item) {
    item.remove();
  });
  const dialog = document.getElementById('archive-modal');
  if (dialog && dialog.open) dialog.close();
  document.body.dispatchEvent(new CustomEvent('sse:card-swapped'));
});
