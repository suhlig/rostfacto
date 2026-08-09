  (function() {
    // Cross-client sync: apply events from the SSE stream, deduplicating the
    // events our own mutations produce (those are also applied via HTMX).
    const slug = document.body.dataset.retroSlug;
    const appliedEventIds = new Set();
    const MAX_APPLIED_IDS = 200;

    // Remember the ids of events our own mutations produced, so the matching
    // SSE events are not applied twice.
    document.body.addEventListener('htmx:afterRequest', function(event) {
      const xhr = event.detail && event.detail.xhr;
      if (!xhr) return;
      const eventId = xhr.getResponseHeader('X-Event-Id');
      if (!eventId) return;
      appliedEventIds.add(eventId);
      while (appliedEventIds.size > MAX_APPLIED_IDS) {
        appliedEventIds.delete(appliedEventIds.values().next().value);
      }
    });
    // Plain fetch responses (timer start/extend) carry the header too.
    document.body.addEventListener('sse:event-applied', function(event) {
      appliedEventIds.add(event.detail.id);
      while (appliedEventIds.size > MAX_APPLIED_IDS) {
        appliedEventIds.delete(appliedEventIds.values().next().value);
      }
    });

    // The HTMX swap and the SSE delivery of the same mutation can arrive in
    // either order, so keep the board free of duplicate cards.
    function removeDuplicateCards() {
      document.querySelectorAll('.item-list').forEach(function(list) {
        const seen = new Set();
        list.querySelectorAll('article.card').forEach(function(card) {
          const id = card.dataset.itemId;
          if (seen.has(id)) {
            card.remove();
          } else {
            seen.add(id);
          }
        });
      });
    }
    document.body.addEventListener('htmx:afterSwap', removeDuplicateCards);

    function cardExists(itemId) {
      return document.querySelector('article.card[data-item-id="' + itemId + '"]') !== null;
    }

    function fetchCardHtml(itemId, onSuccess) {
      fetch('/items/' + itemId, { headers: { Accept: 'text/html' } })
        .then(function(response) {
          if (!response.ok) throw new Error('card fetch failed: ' + response.status);
          return response.text();
        })
        .then(onSuccess)
        .catch(function(error) {
          console.error('SSE: failed to fetch card', itemId, error);
        });
    }

    function notifyCardSwapped() {
      document.body.dispatchEvent(new CustomEvent('sse:card-swapped'));
    }

    // htmx 2.0 binds trigger handlers directly on elements when it processes
    // them, so cards inserted via SSE (not via an HTMX swap) must be handed to
    // htmx explicitly or their buttons stay inert.
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
        // The re-fetched card can be stale: when the SSE event for a status
        // change beats the htmx response, the fetch may complete before the
        // owner's timer auto-start lands, and replacing the card would clobber
        // the running countdown. The deadline is server-authoritative, so carry
        // it over to the incoming badge when it is missing.
        const oldBadge = current.querySelector('.timer-badge');
        const newBadge = replacement.querySelector('.timer-badge');
        if (oldBadge && newBadge &&
            oldBadge.hasAttribute('data-end-at') &&
            !newBadge.hasAttribute('data-end-at') &&
            !newBadge.hasAttribute('data-elapsed')) {
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

    const source = new EventSource('/retro/' + slug + '/events');

    source.addEventListener('ITEM_CREATED', function(event) {
      if (appliedEventIds.has(event.lastEventId)) return;
      const data = parseEvent(event);
      if (!data) return;
      fetchCardHtml(data.item_id, function(html) {
        insertCard(data.category.toLowerCase() + '-items', html);
      });
    });

    source.addEventListener('ITEM_STATUS_CHANGED', function(event) {
      if (appliedEventIds.has(event.lastEventId)) return;
      const data = parseEvent(event);
      if (!data) return;
      // A status change ends the previous timer cycle (completing or
      // cancelling resets the timer columns); the timer module forgets the
      // deadline so a stale render of the next highlight cannot pick it up.
      document.body.dispatchEvent(new CustomEvent('sse:timer-reset', {
        detail: { itemId: data.item_id }
      }));
      fetchCardHtml(data.item_id, function(html) {
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
      const hasActive = Array.from(cards).some(function(card) {
        return !card.classList.contains('completed');
      });
      if (hasActive) return;
      const dialog = document.getElementById('archive-modal');
      if (dialog && !dialog.open && typeof dialog.showModal === 'function') {
        dialog.showModal();
      }
    }

    source.addEventListener('ITEM_UPDATED', function(event) {
      if (appliedEventIds.has(event.lastEventId)) return;
      const data = parseEvent(event);
      if (!data) return;
      const card = document.querySelector('article.card[data-item-id="' + data.item_id + '"]');
      if (!card) return;
      const text = card.querySelector('.card-text');
      if (text) text.textContent = data.text;
    });

    source.addEventListener('ITEM_LIKED', function(event) {
      if (appliedEventIds.has(event.lastEventId)) return;
      const data = parseEvent(event);
      if (!data) return;
      updateLikeCount(data.item_id, data.likes_count);
    });

    source.addEventListener('ITEM_UNLIKED', function(event) {
      if (appliedEventIds.has(event.lastEventId)) return;
      const data = parseEvent(event);
      if (!data) return;
      updateLikeCount(data.item_id, data.likes_count);
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
      document.body.dispatchEvent(new CustomEvent('sse:timer-updated', {
        detail: { itemId: data.item_id, endsAt: endsAt }
      }));
    }

    source.addEventListener('TIMER_ELAPSED', function(event) {
      if (appliedEventIds.has(event.lastEventId)) return;
      const data = parseEvent(event);
      if (!data) return;
      document.body.dispatchEvent(new CustomEvent('sse:timer-elapsed', {
        detail: { itemId: data.item_id }
      }));
    });

    // The retro was archived: clear the board and stop all timers (removing
    // the badges stops their countdowns).
    source.addEventListener('RETRO_ARCHIVED', function(event) {
      if (appliedEventIds.has(event.lastEventId)) return;
      document.querySelectorAll('.item-list article.card').forEach(function(card) {
        card.remove();
      });
      const dialog = document.getElementById('archive-modal');
      if (dialog && dialog.open) dialog.close();
      document.body.dispatchEvent(new CustomEvent('sse:card-swapped'));
    });
  })();

  (function() {
    // Server-authoritative countdown: the deadline comes from the DB
    // (timer_ends_at, rendered as data-end-at), never from a local map, so
    // every client shows the same countdown.
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
      const tick = function() {
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
        const extendBtn = badge.closest('.timer-wrap') ? badge.closest('.timer-wrap').querySelector('.timer-extend') : null;
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
        const extendBtn = badge.closest('.timer-wrap') ? badge.closest('.timer-wrap').querySelector('.timer-extend') : null;
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
        body: params ? params.toString() : ''
      }).then(function(response) {
        const eventId = response.headers.get('X-Event-Id');
        if (eventId) {
          // The response already applied the change; ignore the matching SSE event.
          document.body.dispatchEvent(new CustomEvent('sse:event-applied', { detail: { id: eventId } }));
        }
        if (!response.ok) throw new Error('timer request failed: ' + response.status);
        return response.text();
      }).then(function(html) {
        applyTimerResponseHtml(itemId, html);
      }).catch(function(error) {
        console.error('SSE: timer request failed', itemId, error);
      });
    }

    function startTimerRequest(itemId, durationSeconds) {
      const params = new URLSearchParams();
      params.set('duration', String(durationSeconds));
      postTimerAction(itemId, '/items/' + itemId + '/timer/start', params);
    }

    function extendTimerRequest(itemId) {
      postTimerAction(itemId, '/items/' + itemId + '/timer/extend', null);
    }

    // A timer is started by the client that highlighted the card, so only
    // htmx-driven swaps (our own actions) auto-start; SSE swaps never do.
    // afterRequest fires after the swap (and again on a parent because the
    // request element is detached), so look up the highlighted card in the DOM
    // rather than using the request element.
    document.body.addEventListener('htmx:afterRequest', function(event) {
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
    document.body.addEventListener('sse:timer-updated', function(event) {
      timerDeadlines.set(String(event.detail.itemId), event.detail.endsAt);
      const card = document.querySelector('article.card[data-item-id="' + event.detail.itemId + '"]');
      if (!card) return;
      const badge = card.querySelector('.timer-badge');
      if (badge) setCountdown(badge, event.detail.endsAt);
    });

    // A status change ends the previous timer cycle (completing or cancelling
    // resets the timer columns); forget its deadline so a stale render of the
    // next highlight cannot pick it up.
    document.body.addEventListener('sse:timer-reset', function(event) {
      timerDeadlines.delete(String(event.detail.itemId));
    });

    // The owner's own status-change events are deduplicated, so the highlight
    // request itself is the reliable cycle boundary on this client: a deadline
    // left over from the previous cycle must not block the auto-start below.
    document.body.addEventListener('htmx:beforeRequest', function(event) {
      const elt = (event.detail && (event.detail.requestConfig && event.detail.requestConfig.elt || event.detail.elt)) || null;
      const itemId = elt && elt.dataset ? elt.dataset.itemId : null;
      if (itemId) timerDeadlines.delete(itemId);
    });

    document.body.addEventListener('sse:timer-elapsed', function(event) {
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
    document.body.addEventListener('click', function(e) {
      const button = e.target.closest('.timer-extend');
      if (button) {
        e.stopPropagation();
        const card = button.closest('article.card');
        if (card && card.dataset.itemId) extendTimerRequest(card.dataset.itemId);
      }
    });
  })();

  (function() {
    function isTyping(target) {
      const tag = target.tagName;
      return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || tag === 'BUTTON' || target.isContentEditable;
    }

    function isInsideDialog(target) {
      return target.closest('dialog[open]') !== null;
    }

    document.addEventListener('keydown', function(e) {
      const card = e.target.closest('article.card');
      if (card && e.target === card) {
        if ((e.key === 'Enter' || e.key === ' ') && card.hasAttribute('hx-post')) {
          e.preventDefault();
          card.click();
          return;
        }
        if (e.key === 'Escape' && card.classList.contains('highlighted')) {
          e.preventDefault();
          const cancel = card.querySelector('.card-actions .btn-secondary');
          if (cancel) cancel.click();
          return;
        }
        if (e.key === 'l' || e.key === 'L') {
          e.preventDefault();
          const like = card.querySelector('.like-button');
          if (like) like.click();
          return;
        }
      }

      if (isTyping(e.target) || isInsideDialog(e.target)) return;

      if (e.key === 'n' || e.key === 'N') {
        e.preventDefault();
        const input = document.querySelector('.add-card-input');
        if (input) {
          input.focus();
          input.scrollIntoView({ behavior: 'smooth', block: 'center' });
        }
        return;
      }

      if (e.key === '?') {
        e.preventDefault();
        const dialog = document.getElementById('keyboard-help');
        if (dialog) dialog.showModal();
        return;
      }
    });
  })();

  (function() {
    const section = document.querySelector('.action-items');
    if (!section) return;
    const pool = section.querySelector('#action-items-pool');
    const columns = {
      today: section.querySelector('[data-action-items="today"]'),
      recent: section.querySelector('[data-action-items="recent"]'),
      older: section.querySelector('[data-action-items="older"]')
    };
    const recentHeading = section.querySelector('[data-action-group="recent"]');

    function localDateKey(date) {
      return date.getFullYear() + '-' + String(date.getMonth() + 1).padStart(2, '0') + '-' + String(date.getDate()).padStart(2, '0');
    }

    function displayDate(date) {
      return date.toLocaleDateString(undefined, { month: 'long', day: 'numeric' });
    }

    function groupActionItems() {
      const now = new Date();
      const todayKey = localDateKey(now);
      const items = Array.from(pool.querySelectorAll('.action-item'));
      if (items.length === 0) return;
      const priorDates = items
        .map(item => new Date(item.dataset.createdAt))
        .filter(date => localDateKey(date) !== todayKey)
        .sort((a, b) => b - a);
      const recentKey = priorDates.length ? localDateKey(priorDates[0]) : null;
      section.querySelector('[data-action-group="today"]').textContent = 'Today (' + displayDate(now) + ')';
      recentHeading.textContent = recentKey ? displayDate(priorDates[0]) : '';

      Object.values(columns).forEach(column => { column.replaceChildren(); });
      items.forEach(item => {
        const dateKey = localDateKey(new Date(item.dataset.createdAt));
        const group = dateKey === todayKey ? 'today' : (dateKey === recentKey ? 'recent' : 'older');
        columns[group].appendChild(item);
      });
    }

    document.addEventListener('DOMContentLoaded', groupActionItems);
    document.body.addEventListener('htmx:afterSwap', function(event) {
      if (event.detail && event.detail.target === pool) {
        groupActionItems();
      }
    });
  })();

  (function() {
    // hx-on replacements: the inline event handlers were removed so the page
    // can run under a strict Content-Security-Policy (no unsafe-inline/eval).

    // Clicking a like or edit button inside a created card must not also
    // trigger the card's hx-post (highlight). The guard stops the click from
    // bubbling to the card; htmx attaches its own listener to the button.
    function installClickGuards() {
      document.querySelectorAll('.like-button, .card-text-edit').forEach(function(button) {
        if (button.dataset.clickGuard) return;
        button.dataset.clickGuard = '1';
        button.addEventListener('click', function(event) {
          event.stopPropagation();
        });
      });
    }
    installClickGuards();
    document.body.addEventListener('htmx:afterSettle', installClickGuards);
    document.body.addEventListener('sse:card-swapped', installClickGuards);

    // Reset add-card and action-item forms after a successful submission
    // (replaces hx-on::after-request on those forms).
    document.body.addEventListener('htmx:afterRequest', function(event) {
      const elt = event.detail && event.detail.elt;
      if (!elt || !elt.matches || !event.detail.successful) return;
      if (elt.matches('form.add-card-form, form.action-items-form')) {
        elt.reset();
      }
    });

    // Keyboard shortcuts on card text inputs (replaces hx-on:keydown on the
    // add-card and edit-card textareas): Cmd/Ctrl+Enter submits, Escape
    // cancels an in-progress edit.
    document.addEventListener('keydown', function(event) {
      const target = event.target;
      if (!target || !target.matches) return;
      if (!target.matches('textarea.add-card-input, textarea.edit-card-input')) return;
      if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        const form = target.closest('form');
        if (form && form.requestSubmit) form.requestSubmit();
      } else if (event.key === 'Escape' && target.matches('textarea.edit-card-input')) {
        const cancel = target.closest('form').querySelector('.btn-cancel-edit');
        if (cancel) cancel.click();
      }
    });
  })();
