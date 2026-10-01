// Group action items into dated columns (Today / most recent prior date /
// older). Re-run whenever the pool changes, whether via an HTMX swap or an
// SSE insert (the sync module dispatches sse:action-item-added).

const section = document.querySelector('.action-items');
const pool = section ? section.querySelector('#action-items-pool') : null;
const columns = section
  ? {
    today: section.querySelector('[data-action-items="today"]'),
    recent: section.querySelector('[data-action-items="recent"]'),
    older: section.querySelector('[data-action-items="older"]'),
  }
  : null;
const recentHeading = section ? section.querySelector('[data-action-group="recent"]') : null;

function localDateKey(date) {
  return date.getFullYear() + '-' + String(date.getMonth() + 1).padStart(2, '0') + '-' +
    String(date.getDate()).padStart(2, '0');
}

function displayDate(date) {
  return date.toLocaleDateString(undefined, { month: 'long', day: 'numeric' });
}

function groupActionItems() {
  const now = new Date();
  const todayKey = localDateKey(now);
  // The HTMX add response and the SSE insert can both add the same item
  // (the SSE event may arrive before the response's X-Event-Id is
  // recorded), so keep one node per id: the last in DOM order (the freshest
  // render), mirroring removeDuplicateCards for cards.
  const seen = new Set();
  const latest = new Map();
  Array.from(section.querySelectorAll('.action-item')).forEach(function (item) {
    const id = item.dataset.actionItemId;
    if (seen.has(id)) {
      const older = latest.get(id);
      if (older && older !== item) older.remove();
    }
    latest.set(id, item);
    seen.add(id);
  });
  // Items live in the columns once grouped, so collect from the whole
  // section (pool + columns). Reading only the pool would drop every
  // already-grouped item the next time this runs (e.g. after adding one).
  const items = Array.from(section.querySelectorAll('.action-item'));
  if (items.length === 0) return;
  const priorDates = items
    .map((item) => new Date(item.dataset.createdAt))
    .filter((date) => localDateKey(date) !== todayKey)
    .sort((a, b) => b - a);
  const recentKey = priorDates.length ? localDateKey(priorDates[0]) : null;
  section.querySelector('[data-action-group="today"]').textContent = 'Today (' + displayDate(now) +
    ')';
  recentHeading.textContent = recentKey ? displayDate(priorDates[0]) : '';

  Object.values(columns).forEach((column) => {
    column.replaceChildren();
  });
  items.forEach((item) => {
    const dateKey = localDateKey(new Date(item.dataset.createdAt));
    const group = dateKey === todayKey ? 'today' : (dateKey === recentKey ? 'recent' : 'older');
    columns[group].appendChild(item);
  });
}

if (section) {
  document.addEventListener('DOMContentLoaded', groupActionItems);
  document.body.addEventListener('htmx:afterSwap', function (event) {
    if (event.detail && event.detail.target === pool) {
      groupActionItems();
    }
  });
  // An action item inserted by the SSE sync path lands in the pool; re-group
  // so it joins the existing (already-grouped) items instead of replacing them.
  document.body.addEventListener('sse:action-item-added', groupActionItems);
}
