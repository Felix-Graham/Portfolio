const FALLBACK_SRC = '../assets/fallback-art.png';

const imgEl = document.getElementById('album-art-img');
let lastArtUrl = null;

export function update({ artUrl } = {}) {
  const src = artUrl && artUrl.length ? artUrl : FALLBACK_SRC;
  if (src === lastArtUrl) return;
  lastArtUrl = src;

  // Fade out, swap src, fade back in — avoids a jarring pop on track change.
  imgEl.style.opacity = '0';
  const swap = () => {
    imgEl.onerror = () => {
      imgEl.onerror = null;
      imgEl.src = FALLBACK_SRC;
      lastArtUrl = FALLBACK_SRC;
      imgEl.style.opacity = '1';
    };
    imgEl.onload = () => {
      imgEl.style.opacity = '1';
    };
    imgEl.src = src;
  };
  setTimeout(swap, 120);
}

export function reset() {
  update({ artUrl: null });
}
