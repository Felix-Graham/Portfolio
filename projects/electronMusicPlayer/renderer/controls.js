const btnPrevious = document.getElementById('btn-previous');
const btnPlayPause = document.getElementById('btn-play-pause');
const btnNext = document.getElementById('btn-next');
const iconPlay = document.getElementById('icon-play');
const iconPause = document.getElementById('icon-pause');

export function init() {
  btnPrevious.addEventListener('click', () => window.player.previous());
  btnNext.addEventListener('click', () => window.player.next());
  btnPlayPause.addEventListener('click', () => window.player.playPause());
}

export function setStatus(status) {
  const isPlaying = status === 'Playing';
  iconPlay.style.display = isPlaying ? 'none' : '';
  iconPause.style.display = isPlaying ? '' : 'none';
}
