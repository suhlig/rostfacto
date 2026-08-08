  (function () {
    // Minimal carousel for the landing page: prev/next, dots, keyboard
    // arrows, and autoplay that pauses on hover/focus and respects
    // prefers-reduced-motion.
    var carousel = document.querySelector('.landing-carousel');
    if (!carousel) return;
    var slides = Array.prototype.slice.call(carousel.querySelectorAll('.carousel-slide'));
    var dots = Array.prototype.slice.call(carousel.querySelectorAll('.carousel-dot'));
    var prev = carousel.querySelector('.carousel-prev');
    var next = carousel.querySelector('.carousel-next');
    var AUTOPLAY_MS = 6000;
    var index = 0;
    var timer = null;

    function show(newIndex) {
      index = (newIndex + slides.length) % slides.length;
      slides.forEach(function (slide, i) {
        var active = i === index;
        slide.classList.toggle('is-active', active);
        slide.hidden = !active;
      });
      dots.forEach(function (dot, i) {
        var active = i === index;
        dot.classList.toggle('is-active', active);
        if (active) {
          dot.setAttribute('aria-current', 'true');
        } else {
          dot.removeAttribute('aria-current');
        }
      });
    }

    function startAutoplay() {
      stopAutoplay();
      if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
      timer = window.setInterval(function () { show(index + 1); }, AUTOPLAY_MS);
    }

    function stopAutoplay() {
      if (timer) window.clearInterval(timer);
      timer = null;
    }

    function advance(step) {
      show(index + step);
      startAutoplay();
    }

    prev.addEventListener('click', function () { advance(-1); });
    next.addEventListener('click', function () { advance(1); });
    dots.forEach(function (dot, i) {
      dot.addEventListener('click', function () { advance(i - index); });
    });

    carousel.addEventListener('mouseenter', stopAutoplay);
    carousel.addEventListener('mouseleave', startAutoplay);
    carousel.addEventListener('focusin', stopAutoplay);
    carousel.addEventListener('focusout', startAutoplay);
    carousel.addEventListener('keydown', function (event) {
      if (event.key === 'ArrowLeft') { advance(-1); event.preventDefault(); }
      if (event.key === 'ArrowRight') { advance(1); event.preventDefault(); }
    });

    show(0);
    startAutoplay();
  })();
