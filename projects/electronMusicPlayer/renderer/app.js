import * as albumArt from './albumArt.js';
import * as controls from './controls.js';
import { setTargetFrame, drawFrame } from './visualizer.js';

const canvas = document.getElementById('visualizer');
const ctx = canvas.getContext('2d');
const ringStack = document.getElementById('ring-stack');
const titleEl = document.getElementById('track-title');
const artistEl = document.getElementById('track-artist');

const DEFAULT_TITLE = 'No player detected';
const DEFAULT_ARTIST = 'Play something to get started';

function resizeCanvas() {
  const rect = ringStack.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = rect.width * dpr;
  canvas.height = rect.height * dpr;
  canvas.style.width = `${rect.width}px`;
  canvas.style.height = `${rect.height}px`;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
}

function renderLoop() {
  const rect = ringStack.getBoundingClientRect();
  drawFrame(ctx, rect.width, rect.height);
  requestAnimationFrame(renderLoop);
}

function showDefaultState() {
  titleEl.textContent = DEFAULT_TITLE;
  artistEl.textContent = DEFAULT_ARTIST;
  albumArt.reset();
  controls.setStatus('Paused');
}

function init() {
  resizeCanvas();
  window.addEventListener('resize', resizeCanvas);

  controls.init();
  showDefaultState();

  window.player.onMetadata(({ title, artist, artUrl }) => {
    titleEl.textContent = title && title.length ? title : DEFAULT_TITLE;
    artistEl.textContent = artist && artist.length ? artist : '';
    albumArt.update({ artUrl });
  });

  window.player.onStatus((status) => {
    controls.setStatus(status);
  });

  window.player.onNoPlayer(() => {
    showDefaultState();
  });

  window.player.onVizFrame((bins) => {
    setTargetFrame(bins);
  });

  requestAnimationFrame(renderLoop);
}

init();
