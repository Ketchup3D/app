// eframe sets the inline cursor after each frame. Override only its "none"
// state, which is reserved for tool cursors; all native UI cursors still work.
let currentKey;
let style;
const cursors = new Map();
export function setToolCursor(key, geometry, hotspotX, hotspotY, scale) {
  const canvas = document.getElementById("ketchup-canvas");
  if (!canvas) return false;
  if (currentKey === key && style?.isConnected) return true;
  let cursor = cursors.get(key);
  if (!cursor) {
    const image = document.createElement("canvas");
    const resolution = 2;
    image.width = image.height = Math.ceil(64 * scale * resolution);
    const ctx = image.getContext("2d");
    if (!ctx) return false;
    ctx.scale(scale * resolution, scale * resolution);
    for (const command of JSON.parse(geometry)) {
      const [kind, position, size, detail, fill, stroke, width] = command;
      ctx.beginPath();
      if (kind === "path") {
        for (const [i, p] of position.entries()) {
          if (i === 0) ctx.moveTo(...p); else ctx.lineTo(...p);
        }
        if (size) ctx.closePath();
        ctx.fillStyle = detail;
        ctx.strokeStyle = fill;
        ctx.lineWidth = stroke || 1;
        ctx.fill();
        if (stroke > 0) ctx.stroke();
      } else {
        if (kind === "ellipse") ctx.ellipse(...position, ...size, detail, 0, Math.PI * 2);
        else ctx.roundRect(...position, ...size, detail);
        ctx.fillStyle = fill;
        ctx.strokeStyle = stroke;
        ctx.lineWidth = width || 1;
        ctx.fill();
        if (width > 0) ctx.stroke();
      }
    }
    // A CSS image-set preserves sharp icons without doubling their visual size.
    cursor = `image-set(url("${image.toDataURL()}") 2x) ${Math.round(hotspotX * scale)} ${Math.round(hotspotY * scale)}, auto`;
    if (!CSS.supports("cursor", cursor)) return false;
    cursors.set(key, cursor);
  }
  if (!style?.isConnected) {
    style = document.createElement("style");
    document.head.appendChild(style);
  }
  style.textContent = `#ketchup-canvas[style*="cursor: none"] { cursor: ${cursor} !important; }`;
  currentKey = key;
  return true;
}
