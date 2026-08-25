// Draws btop-style dots around a ring. Values are smoothed frame-to-frame
// so cava's ~30fps updates don't look like they're snapping.

const SMOOTHING = 0.22; // higher = snappier, lower = more languid
const DOT_MIN_RADIUS = 1.5;
const DOT_MAX_RADIUS = 4.5;
const RING_RADIUS_RATIO = 0.92; // relative to canvas half-width
const BOTTOM_GAP_DEGREES = 100; // arc hidden behind the control bar

let current = []; // smoothed, currently-displayed bin values
let target = [];  // latest values received from the main process

export function setTargetFrame(bins) {
  target = bins;
  if (current.length !== target.length) {
    current = target.slice();
  }
}

export function drawFrame(ctx, canvasWidth, canvasHeight) {
  ctx.clearRect(0, 0, canvasWidth, canvasHeight);
  if (!target.length) return;

  const cx = canvasWidth / 2;
  const cy = canvasHeight / 2;
  const ringRadius = Math.min(cx, cy) * RING_RADIUS_RATIO;

  const count = target.length;
  const gapRad = (BOTTOM_GAP_DEGREES * Math.PI) / 180;
  const usableArc = Math.PI * 2 - gapRad;
  // Start just after the gap (gap is centered at the bottom, angle = 90deg)
  const startAngle = Math.PI / 2 + gapRad / 2;

  for (let i = 0; i < count; i++) {
    current[i] += (target[i] - current[i]) * SMOOTHING;

    const t = i / count;
    const angle = startAngle + t * usableArc;

    const x = cx + ringRadius * Math.cos(angle);
    const y = cy + ringRadius * Math.sin(angle);

    const amplitude = Math.min(current[i] / 255, 1);
    const dotRadius = DOT_MIN_RADIUS + amplitude * (DOT_MAX_RADIUS - DOT_MIN_RADIUS);

    ctx.beginPath();
    ctx.arc(x, y, dotRadius, 0, Math.PI * 2);
    ctx.fillStyle = `rgba(138, 255, 128, ${0.35 + amplitude * 0.65})`;
    ctx.fill();
  }
}
