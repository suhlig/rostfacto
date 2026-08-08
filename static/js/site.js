// Site-wide UI behavior. Inline event handlers were removed from the
// templates so the pages can run under a strict Content-Security-Policy
// (no unsafe-inline), so all dialog and menu wiring lives here.

(function () {
  // Dialog open/close buttons: any element with data-open-dialog or
  // data-close-dialog. The value of data-open-dialog is the dialog's id;
  // data-close-dialog closes the closest ancestor <dialog>.
  document.addEventListener('click', function (event) {
    var opener = event.target.closest('[data-open-dialog]');
    if (opener) {
      var dialog = document.getElementById(opener.getAttribute('data-open-dialog'));
      if (dialog && typeof dialog.showModal === 'function') dialog.showModal();
      return;
    }
    var closer = event.target.closest('[data-close-dialog]');
    if (closer) {
      var dialog = closer.closest('dialog');
      if (dialog) dialog.close();
    }
  });

  // htmx delete forms (retro rows, action items): close their confirmation
  // dialog after a successful request.
  document.body.addEventListener('htmx:afterRequest', function (event) {
    var elt = event.detail && event.detail.elt;
    if (!elt || !elt.matches || !event.detail.successful) return;
    if (elt.matches('form[hx-delete]')) {
      var dialog = elt.closest('dialog');
      if (dialog && dialog.open) dialog.close();
    }
  });

  // Account menu open/close (replaces the per-menu inline script).
  document.querySelectorAll('.account-menu').forEach(function (menu) {
    var button = menu.querySelector('button');
    if (!button) return;

    menu.addEventListener('click', function (event) {
      event.stopPropagation();
    });
    button.addEventListener('click', function () {
      menu.classList.toggle('is-open');
    });
    document.addEventListener('click', function () {
      menu.classList.remove('is-open');
    });
  });

  // Archive this retro: confirm when unaddressed cards remain, otherwise
  // submit the direct archive form (replaces the inline openArchiveMenuDialog
  // script).
  function openArchiveMenuDialog(event, retroId) {
    event.preventDefault();
    var unaddressed = document.querySelectorAll('article.card:not(.completed)');
    if (unaddressed.length > 0) {
      var dialog = document.getElementById('archive-confirm-' + retroId);
      var message = document.getElementById('archive-confirm-message-' + retroId);
      message.textContent = 'There ' + (unaddressed.length === 1 ? 'is' : 'are') + ' ' +
        unaddressed.length + ' unaddressed card' + (unaddressed.length === 1 ? '' : 's') +
        '. Are you sure you want to archive them?';
      dialog.showModal();
    } else {
      document.getElementById('archive-direct-form-' + retroId).submit();
    }
  }

  document.querySelectorAll('[data-archive-menu-retro]').forEach(function (link) {
    var retroId = link.getAttribute('data-archive-menu-retro');

    link.addEventListener('click', function (event) {
      event.preventDefault();
      if (link.classList.contains('disabled')) return;
      openArchiveMenuDialog(event, retroId);
    });

    function updateArchiveLink() {
      var hasCards = document.querySelectorAll('article.card').length > 0;
      if (hasCards) {
        link.classList.remove('disabled');
      } else {
        link.classList.add('disabled');
      }
    }

    document.addEventListener('DOMContentLoaded', updateArchiveLink);
    document.body.addEventListener('htmx:afterSettle', updateArchiveLink);
    updateArchiveLink();
  });
})();
