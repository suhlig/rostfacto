// New-retro form: auto-fill the slug from the title while typing, until the
// user edits the slug themselves. The slug field carries htmx attributes that
// check availability; dispatching an `input` event after a programmatic fill
// makes htmx run that check for the generated slug too.
(function () {
  const title = document.getElementById('title');
  const slug = document.getElementById('slug');
  if (!title || !slug) return;

  let slugTouched = false;
  let fillingSlug = false;

  function slugify(value) {
    return value
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 255);
  }

  title.addEventListener('input', function () {
    if (slugTouched) return;
    fillingSlug = true;
    slug.value = slugify(title.value);
    slug.dispatchEvent(new Event('input', { bubbles: true }));
    fillingSlug = false;
  });

  slug.addEventListener('input', function () {
    if (fillingSlug) return;
    slugTouched = true;
  });
})();
