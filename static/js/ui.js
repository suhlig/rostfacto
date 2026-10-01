// Inline-handler replacements that the strict CSP forbids: click guards on
// buttons inside a card, and form resets after a successful submission.

// Clicking a like or edit button inside a created card must not also
// trigger the card's hx-post (highlight). The guard stops the click from
// bubbling to the card; htmx attaches its own listener to the button.
function installClickGuards() {
  document.querySelectorAll('.like-button, .card-text-edit').forEach(function (button) {
    if (button.dataset.clickGuard) return;
    button.dataset.clickGuard = '1';
    button.addEventListener('click', function (event) {
      event.stopPropagation();
    });
  });
}
installClickGuards();
document.body.addEventListener('htmx:afterSettle', installClickGuards);
document.body.addEventListener('sse:card-swapped', installClickGuards);

// Reset add-card and action-item forms after a successful submission
// (replaces hx-on::after-request on those forms).
document.body.addEventListener('htmx:afterRequest', function (event) {
  const elt = event.detail && event.detail.elt;
  if (!elt || !elt.matches || !event.detail.successful) return;
  if (elt.matches('form.add-card-form, form.action-items-form')) {
    elt.reset();
  }
});
