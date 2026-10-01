// Keyboard shortcuts for the board: global keys (n, ?, Enter/Space/Escape/l
// on a focused card) and the textarea keys (Cmd/Ctrl+Enter submits, Escape
// cancels an edit). These replace the inline hx-on handlers the strict CSP
// forbids.

function isTyping(target) {
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || tag === 'BUTTON' ||
    target.isContentEditable;
}

function isInsideDialog(target) {
  return target.closest('dialog[open]') !== null;
}

document.addEventListener('keydown', function (e) {
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

// Keyboard shortcuts on card text inputs (replaces hx-on:keydown on the
// add-card and edit-card textareas, which action item editing shares):
// Cmd/Ctrl+Enter submits, Escape cancels an in-progress edit.
document.addEventListener('keydown', function (event) {
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
