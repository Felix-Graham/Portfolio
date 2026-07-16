// ─────────────────────────────────────────────
//  MAIN.JS
//  Nav, hero, and footer are now static HTML
//  (see index.html). This file only handles:
//    - theme (dark/light) toggle
//    - dynamic project list rendering
// ─────────────────────────────────────────────

import { projects } from './data/projects.js';
import { initFallingGlyphs } from './effects.js';

// ── SVG icon library (only icons still needed by JS-rendered markup) ──
const icons = {
  chevronDown: `<svg class="chevron" viewBox="0 0 24 24"><polyline points="6 9 12 15 18 9"/></svg>`,
  externalLink: `<svg viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>`,
  github: `<svg viewBox="0 0 24 24"><path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22"/></svg>`,
  download: `<svg viewBox="0 0 24 24"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>`,
  fileText: `<svg viewBox="0 0 24 24"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10 9 9 9 8 9"/></svg>`,
  photo: `<svg viewBox="0 0 24 24"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`,
};

function getIcon(name) {
  const map = {
    'external-link': icons.externalLink,
    'github':        icons.github,
    'download':      icons.download,
    'file-text':     icons.fileText,
  };
  return map[name] || icons.externalLink;
}

// ── Theme ─────────────────────────────────────
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

// ── Render projects ───────────────────────────
function renderProjects() {
  const container = document.querySelector('.projects__list');

  const years = projects.map(p => parseInt(p.year)).filter(Boolean);
  if (years.length) {
    const min = Math.min(...years);
    const max = Math.max(...years);
    const rangeEl = document.querySelector('.projects__year-range');
    if (rangeEl) rangeEl.textContent = min === max ? `${max}` : `${min} – ${max}`;
  }

  projects.forEach((project, i) => {
    const index = String(i + 1).padStart(2, '0');
    const el = document.createElement('article');
    el.className = 'project';
    el.setAttribute('aria-label', project.title);

    const imageHTML = project.image
      ? `<img class="project__img" src="${project.image}" alt="${project.title}" loading="lazy">`
      : `<div class="project__placeholder">
           ${icons.photo}
           <span>Project image</span>
         </div>`;

    const linksHTML = project.links
      .map(l => `
        <a href="${l.href}" class="project__link" target="_blank" rel="noopener noreferrer">
          ${getIcon(l.icon)}
          ${l.label}
        </a>`)
      .join('');

    const tagsHTML = project.tags
      .map(t => `<span class="tag">${t}</span>`)
      .join('');

    el.innerHTML = `
      <div class="project__summary" role="button" tabindex="0" aria-expanded="false">
        <span class="project__index">${index}</span>
        <div class="project__meta">
          <p class="project__title">${project.title}</p>
          <p class="project__sub">${project.sub}</p>
        </div>
        <div class="project__right">
          ${tagsHTML}
          ${icons.chevronDown}
        </div>
      </div>
      <div class="project__expand" role="region">
        <div class="project__expand-inner">
          <div class="project__body">
            <div class="project__image-col">${imageHTML}</div>
            <div class="project__text-col">
              <div>
                <p class="project__year">${project.year}</p>
                <p class="project__description">${project.description}</p>
              </div>
              <div class="project__links">${linksHTML}</div>
            </div>
          </div>
        </div>
      </div>
    `;

    const summary = el.querySelector('.project__summary');
    const toggle  = () => {
      const isOpen = el.classList.contains('is-open');
      document.querySelectorAll('.project.is-open').forEach(p => {
        p.classList.remove('is-open');
        p.querySelector('.project__summary').setAttribute('aria-expanded', 'false');
      });
      if (!isOpen) {
        el.classList.add('is-open');
        summary.setAttribute('aria-expanded', 'true');
      }
    };

    summary.addEventListener('click', toggle);
    summary.addEventListener('keydown', e => {
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(); }
    });

    container.appendChild(el);
  });
}

function initHeroGlyphs() {
  const layer = document.querySelector('.hero__glyphs');
  initFallingGlyphs(layer);
}

// ── Boot ──────────────────────────────────────
initTheme();
wireThemeButton();
renderProjects();
initHeroGlyphs();
