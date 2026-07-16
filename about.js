// ─────────────────────────────────────────────
//  ABOUT.JS
//  Renders /about.html entirely from
//  data/about-config.js. Handles:
//    - theme toggle (same pattern as main.js)
//    - index nav + scroll-spy
//    - sticky-stacked section rendering
//    - per-section dropdowns (text or image)
//    - optional falling-glyph backgrounds
// ─────────────────────────────────────────────

import { aboutPage } from './data/about-config.js';
import { initFallingGlyphs } from './effects.js';

const chevronSVG = `<svg class="chevron" viewBox="0 0 24 24"><polyline points="6 9 12 15 18 9"/></svg>`;

// ── Theme (same as main.js — kept in sync manually since these
//    are two separate entry points, not a shared framework) ──
const THEME_KEY = 'pf-theme';

function initTheme() {
  const saved = localStorage.getItem(THEME_KEY);
  const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
  const theme = saved || (prefersDark ? 'dark' : 'light');
  document.documentElement.setAttribute('data-theme', theme);
}

function toggleTheme() {
  const current = document.documentElement.getAttribute('data-theme');
  const next    = current === 'dark' ? 'light' : 'dark';
  document.documentElement.setAttribute('data-theme', next);
  localStorage.setItem(THEME_KEY, next);
}

function wireThemeButton() {
  document.getElementById('themeBtn').addEventListener('click', toggleTheme);
}

// ── Index nav ─────────────────────────────────
function renderIndex() {
  const nav = document.getElementById('aboutIndex');
  nav.innerHTML = aboutPage.index
    .map(item => `<a href="#${item.id}" data-target="${item.id}">${item.label}</a>`)
    .join('');
}

// ── Image column helper ──────────────────────
// Only called when a side actually has images — an empty/absent
// array is now handled one level up, in renderSections, by simply
// not rendering that column at all.
function renderImageColumn(images) {
  return images.map(img => img.src
    ? `<img class="about-image" src="${img.src}" alt="${img.alt || ''}" loading="lazy">`
    : `<div class="about-image about-image--placeholder"><span>${img.alt || 'Image'}</span></div>`
  ).join('');
}

// Decides how many grid columns a section needs based on which
// sides actually have images. Returns one of:
//   'both'  — image | text | image   (images pushed to the edges)
//   'left'  — image | text           (text widens to fill the right)
//   'right' — text | image           (text widens to fill the left)
//   'none'  — text                   (text takes the full centre column)
function getLayoutMode(section) {
  const hasLeft  = Boolean(section.images?.left?.length);
  const hasRight = Boolean(section.images?.right?.length);
  if (hasLeft && hasRight) return 'both';
  if (hasLeft) return 'left';
  if (hasRight) return 'right';
  return 'none';
}

// Turns a single 'MM-YYYY' entry into a sortable YYYYMM number.
// Returns 0 (sorts last) for anything unparseable.
function parseMonthYear(str) {
  const match = /^(\d{2})-(\d{4})$/.exec((str || '').trim());
  if (!match) return 0;
  const [, mm, yyyy] = match;
  return parseInt(yyyy, 10) * 100 + parseInt(mm, 10);
}

// A timeline item's `date` can be either:
//   'MM-YYYY'                 — a single point in time
//   'MM-YYYY -- MM-YYYY'      — a period (start -- end), e.g. for
//                                career history / role durations
// For ranges, the END date is used for "most recent" sorting —
// an ongoing/recent role should still surface near the top even if
// it started a while ago.
function getSortKey(dateStr) {
  const parts = (dateStr || '').split(/\s*--\s*/);
  return parseMonthYear(parts[parts.length - 1]);
}

// Renders a `type: 'timeline'` dropdown. `dropdown.items` can hold
// any number of entries — they're sorted newest-first by `date`.
// Only the top `dropdown.limit` (default 3) show initially; if more
// exist, a "+N more" toggle reveals the rest in place.
function renderTimeline(dropdown) {
  const limit = dropdown.limit || 3;
  const sorted = [...(dropdown.items || [])]
    .sort((a, b) => getSortKey(b.date) - getSortKey(a.date));
  const visible = sorted.slice(0, limit);
  const hidden  = sorted.slice(limit);

  const fontClass = `about-dropdown__timeline--${dropdown.font || 'mono'}`;

  const renderItems = list => list.map(item => `
        <li class="about-dropdown__timeline-item">
          <span class="about-dropdown__timeline-date">${item.date || ''}</span>
          <span class="about-dropdown__timeline-title">${item.title}</span>
        </li>`).join('');

  const moreHTML = hidden.length ? `
    <button class="about-dropdown__timeline-more" aria-expanded="false" data-more-count="${hidden.length}">
      <span class="about-dropdown__timeline-more-label">+ ${hidden.length} more</span>
      <span class="chevron-wrap">${chevronSVG}</span>
    </button>
    <div class="about-dropdown__timeline-more-panel">
      <div class="about-dropdown__timeline-more-inner">
        <ul class="about-dropdown__timeline ${fontClass}">
          ${renderItems(hidden)}
        </ul>
      </div>
    </div>` : '';

  return `
    <ul class="about-dropdown__timeline ${fontClass}">
      ${renderItems(visible)}
    </ul>
    ${moreHTML}`;
}

// ── Dropdown helper ───────────────────────────
function renderDropdown(dropdown) {
  let body;
  if (dropdown.type === 'image') {
    body = `<div class="about-dropdown__gallery">${dropdown.images.map(img => renderImageColumn([img])).join('')}</div>`;
  } else if (dropdown.type === 'timeline') {
    body = renderTimeline(dropdown);
  } else {
    body = `<p class="about-dropdown__text about-dropdown__text--${dropdown.font || 'serif'}">${dropdown.content}</p>`;
  }

  return `
    <button class="about-dropdown__toggle" aria-expanded="false">
      <span>${dropdown.label}</span>
      <span class="chevron-wrap">${chevronSVG}</span>
    </button>
    <div class="about-dropdown__panel">
      <div class="about-dropdown__inner">${body}</div>
    </div>`;
}

function wireDropdown(wrapper) {
  const toggle = wrapper.querySelector('.about-dropdown__toggle');
  const panel  = wrapper.querySelector('.about-dropdown__panel');
  if (toggle && panel) {
    toggle.addEventListener('click', () => {
      const open = panel.classList.contains('is-open');
      panel.classList.toggle('is-open', !open);
      toggle.setAttribute('aria-expanded', String(!open));
    });
  }

  // Timeline's own "+N more" toggle, nested inside the panel above —
  // wired separately since a section can have one without the other.
  const moreBtn   = wrapper.querySelector('.about-dropdown__timeline-more');
  const morePanel = wrapper.querySelector('.about-dropdown__timeline-more-panel');
  if (moreBtn && morePanel) {
    const hiddenCount = moreBtn.dataset.moreCount;
    const label = moreBtn.querySelector('.about-dropdown__timeline-more-label');
    moreBtn.addEventListener('click', () => {
      const open = morePanel.classList.contains('is-open');
      morePanel.classList.toggle('is-open', !open);
      moreBtn.setAttribute('aria-expanded', String(!open));
      if (label) label.textContent = open ? `+ ${hiddenCount} more` : 'Show less';
    });
  }
}

// ── Sections ──────────────────────────────────
function renderSections() {
  const container = document.getElementById('aboutSections');

  aboutPage.sections.forEach((section, i) => {
    const wrapper = document.createElement('div');
    wrapper.className = 'about-section';
    wrapper.id = section.id;
    wrapper.style.setProperty('--scroll-height', `${section.scrollHeight || 150}vh`);
    wrapper.style.setProperty('--stack-index', i + 1);

    const layoutMode = getLayoutMode(section);
    const hasLeft  = layoutMode === 'both' || layoutMode === 'left';
    const hasRight = layoutMode === 'both' || layoutMode === 'right';

    wrapper.innerHTML = `
      <div class="about-section__sticky about-section__sticky--${layoutMode}">
        ${section.glyphs ? '<div class="about-section__glyphs" aria-hidden="true"></div>' : ''}
        ${hasLeft ? `
        <div class="about-section__col about-section__col--left">
          ${renderImageColumn(section.images.left)}
        </div>` : ''}
        <div class="about-section__col about-section__col--center">
          <p class="about-section__eyebrow">${section.eyebrow}</p>
          <h2 class="about-section__heading">${section.heading}</h2>
          <p class="about-section__body">${section.body}</p>
          ${section.dropdown ? renderDropdown(section.dropdown) : ''}
        </div>
        ${hasRight ? `
        <div class="about-section__col about-section__col--right">
          ${renderImageColumn(section.images.right)}
        </div>` : ''}
      </div>
    `;

    container.appendChild(wrapper);

    if (section.glyphs) {
      initFallingGlyphs(wrapper.querySelector('.about-section__glyphs'));
    }
    if (section.dropdown) {
      wireDropdown(wrapper);
    }
  });
}

// ── Scroll-spy: highlight the current section in the index nav ──
function wireScrollSpy() {
  const links = document.querySelectorAll('.about-index a');
  const sections = document.querySelectorAll('.about-section');
  const linkByTarget = new Map();
  links.forEach(l => linkByTarget.set(l.dataset.target, l));

  const observer = new IntersectionObserver(entries => {
    entries.forEach(entry => {
      if (!entry.isIntersecting) return;
      const link = linkByTarget.get(entry.target.id);
      if (!link) return;
      links.forEach(l => l.classList.remove('is-active'));
      link.classList.add('is-active');
    });
  }, { rootMargin: '-45% 0px -45% 0px', threshold: 0 });

  sections.forEach(s => observer.observe(s));
}

// ── Boot ──────────────────────────────────────
initTheme();
wireThemeButton();
renderIndex();
renderSections();
wireScrollSpy();
