const { listen } = window.__TAURI__.event;

const overlay = document.getElementById("overlay");
const textNode = document.getElementById("text");
const canvas = document.getElementById("wave");
const ctx = canvas.getContext("2d");

const state = {
  visible: false,
  amplitude: 0.04,
  targetAmplitude: 0.04,
  text: "",
  phase: 0,
};

function drawWavePath(sign = 1) {
  const w = canvas.width;
  const h = canvas.height;
  const centerX = w / 2;
  const centerY = h * 0.48;
  const maxAmp = h * 0.22;

  ctx.beginPath();
  for (let offset = 0; offset <= centerX; offset += 2) {
    const distance = offset / centerX;
    const envelope = Math.pow(1 - distance, 1.5);
    const sineA = Math.sin(state.phase * 0.07 + offset * 0.07) * 0.62;
    const sineB = Math.sin(state.phase * 0.03 + offset * 0.025) * 0.38;
    const value = (sineA + sineB) * envelope * maxAmp * state.amplitude;

    const rightX = centerX + offset;
    const leftX = centerX - offset;
    const y = centerY + sign * value;

    if (offset === 0) {
      ctx.moveTo(rightX, y);
    } else {
      ctx.lineTo(rightX, y);
    }

    if (offset > 0) {
      ctx.lineTo(leftX, y);
    }
  }
  ctx.stroke();
}

function render() {
  state.phase += 1;
  state.amplitude += (state.targetAmplitude - state.amplitude) * 0.2;

  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = 2.5;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";

  drawWavePath(1);
  drawWavePath(-1);

  requestAnimationFrame(render);
}

function showOverlay() {
  state.visible = true;
  overlay.classList.remove("hidden");
  requestAnimationFrame(() => {
    overlay.classList.add("visible");
  });
}

function hideOverlay() {
  state.visible = false;
  state.targetAmplitude = 0.02;
  overlay.classList.remove("visible");
  setTimeout(() => {
    if (!state.visible) {
      overlay.classList.add("hidden");
    }
  }, 190);
}

listen("overlay-show", showOverlay);
listen("overlay-hide", hideOverlay);
listen("overlay-reset", () => {
  state.text = "";
  textNode.textContent = "";
  state.targetAmplitude = 0.04;
});
listen("overlay-text", ({ payload }) => {
  state.text += payload;
  textNode.textContent = state.text;
});
listen("overlay-wave", ({ payload }) => {
  const level = Number(payload) || 0;
  state.targetAmplitude = Math.min(0.95, 0.08 + level * 1.35);
});

render();
