// ─────────────────────────────────────────────
//  EFFECTS.JS
//  Small reusable visual effects. Currently just
//  the falling-glyph background used on the hero
//  and on select about-page sections.
// ─────────────────────────────────────────────

// Spawns falling monospace characters inside `container`.
// container must be position:relative (or similar) with overflow:hidden —
// this function only appends absolutely-positioned spans to it.
export function initFallingGlyphs(container, options = {}) {
  if (!container) return () => {};

  const {
    chars       = ['0', '1', '{', '}', '/', '<', '>', '_', '#'],
    interval    = 450,   // ms between spawns
    minDuration = 4,     // seconds
    maxDuration = 8,
    density     = 1,     // glyphs spawned per interval tick
  } = options;

  function spawn() {
    const span = document.createElement('span');
    span.className = 'fall-glyph';
    span.textContent = chars[Math.floor(Math.random() * chars.length)];
    span.style.left = `${Math.random() * 100}%`;
    const duration = minDuration + Math.random() * (maxDuration - minDuration);
    span.style.animationDuration = `${duration}s`;
    container.appendChild(span);
    setTimeout(() => span.remove(), duration * 1000);
  }

  const timer = setInterval(() => {
    for (let i = 0; i < density; i++) spawn();
  }, interval);

  // Seed a few immediately so the effect isn't empty on load
  for (let i = 0; i < 6; i++) setTimeout(spawn, i * 300);

  // Return a cleanup handle in case a caller ever needs to stop it
  // (e.g. removing a section from the DOM)
  return () => clearInterval(timer);
}
