const tauri = window.__TAURI__;
const invoke = tauri?.core?.invoke;
const listen = tauri?.event?.listen;

const elements = {
  tools: document.querySelector("#tools"),
  brushPreset: document.querySelector("#brush-preset"),
  color: document.querySelector("#paint-color"),
  colorLabel: document.querySelector("#color-label"),
  brushSize: document.querySelector("#brush-size"),
  sizeOutput: document.querySelector("#size-output"),
  smoothing: document.querySelector("#smoothing"),
  smoothingOutput: document.querySelector("#smoothing-output"),
  undo: document.querySelector("#undo"),
  redo: document.querySelector("#redo"),
  fit: document.querySelector("#fit"),
  addLayer: document.querySelector("#add-layer"),
  deleteLayer: document.querySelector("#delete-layer"),
  layers: document.querySelector("#layers"),
  saveSettings: document.querySelector("#save-settings"),
  reloadSettings: document.querySelector("#reload-settings"),
  resetBrush: document.querySelector("#reset-brush"),
  message: document.querySelector("#message"),
};

let state;
let backgroundEditStart;
const pendingCommands = new Map();
let commandFrame;

function dispatch(command) {
  if (!invoke) {
    showLocalError("Tauri bridge is unavailable");
    return Promise.resolve();
  }
  return invoke("dispatch", { command }).catch((error) => {
    showLocalError(String(error));
  });
}

function coalesce(key, command) {
  pendingCommands.set(key, command);
  if (commandFrame !== undefined) return;
  commandFrame = requestAnimationFrame(() => {
    commandFrame = undefined;
    const commands = [...pendingCommands.values()];
    pendingCommands.clear();
    for (const pending of commands) void dispatch(pending);
  });
}

function render(next) {
  state = next;
  for (const button of elements.tools.querySelectorAll("[data-tool]")) {
    button.classList.toggle("active", button.dataset.tool === next.tool);
  }

  replaceOptions(
    elements.brushPreset,
    next.brushes.map((brush) => [brush.id, brush.name]),
    next.activeBrush,
  );

  const backgroundSelected = next.layers.selection.type === "background";
  elements.colorLabel.textContent = backgroundSelected ? "Background" : "Color";
  elements.color.value = backgroundSelected
    ? floatRgbToHex(next.layers.backgroundColor)
    : rgbaToHex(next.brush.color);
  elements.brushSize.disabled = backgroundSelected;
  elements.brushPreset.disabled = backgroundSelected;
  elements.smoothing.disabled = backgroundSelected;

  if (!elements.brushSize.matches(":active")) {
    elements.brushSize.min = String(next.brush.minimumSize);
    elements.brushSize.max = String(next.brush.maximumSize);
    elements.brushSize.value = String(next.brush.size);
  }
  elements.sizeOutput.textContent = `${Math.round(next.brush.size)} px`;
  if (!elements.smoothing.matches(":active")) {
    elements.smoothing.value = String(next.smoothingStrength);
  }
  elements.smoothingOutput.textContent = `${Math.round(next.smoothingStrength * 100)}%`;

  elements.undo.disabled = !next.canUndo;
  elements.redo.disabled = !next.canRedo;
  elements.deleteLayer.disabled = !next.canDeleteLayer;
  renderLayers(next.layers);
  renderMessage(next.message);
}

function replaceOptions(select, options, selected) {
  const signature = options.map(([value, label]) => `${value}:${label}`).join("|");
  if (select.dataset.signature !== signature) {
    select.replaceChildren(
      ...options.map(([value, label]) => {
        const option = document.createElement("option");
        option.value = value;
        option.textContent = label;
        return option;
      }),
    );
    select.dataset.signature = signature;
  }
  select.value = selected;
}

function renderLayers(layers) {
  const selected = layers.selection;
  const entries = [...layers.layers]
    .reverse()
    .map((layer) => ({
      label: layer.name,
      active: selected.type === "paint" && selected.id === layer.id,
      command: { type: "selectLayer", id: layer.id },
    }));
  entries.push({
    label: "Background",
    active: selected.type === "background",
    command: { type: "selectBackground" },
  });
  elements.layers.replaceChildren(
    ...entries.map((entry) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = `layer${entry.active ? " active" : ""}`;
      button.textContent = entry.label;
      button.role = "option";
      button.ariaSelected = String(entry.active);
      button.addEventListener("click", () => void dispatch(entry.command));
      return button;
    }),
  );
}

function renderMessage(message) {
  if (!message) {
    elements.message.hidden = true;
    return;
  }
  elements.message.hidden = false;
  elements.message.classList.toggle("error", message.isError);
  elements.message.textContent = message.text;
}

function showLocalError(text) {
  renderMessage({ text, isError: true });
}

function rgbaToHex(color) {
  return `#${color
    .slice(0, 3)
    .map((value) => Math.round(value).toString(16).padStart(2, "0"))
    .join("")}`;
}

function floatRgbToHex(color) {
  return rgbaToHex(color.map((value) => value * 255));
}

function hexToRgba(value) {
  return [
    Number.parseInt(value.slice(1, 3), 16),
    Number.parseInt(value.slice(3, 5), 16),
    Number.parseInt(value.slice(5, 7), 16),
    255,
  ];
}

elements.tools.addEventListener("click", (event) => {
  const button = event.target.closest("[data-tool]");
  if (button) void dispatch({ type: "setTool", tool: button.dataset.tool });
});
elements.brushPreset.addEventListener("change", () => {
  void dispatch({ type: "selectBrush", id: elements.brushPreset.value });
});
elements.brushSize.addEventListener("input", () => {
  const size = Number(elements.brushSize.value);
  elements.sizeOutput.textContent = `${Math.round(size)} px`;
  coalesce("brush-size", { type: "setBrushSize", size });
});
elements.smoothing.addEventListener("input", () => {
  elements.smoothingOutput.textContent = `${Math.round(
    Number(elements.smoothing.value) * 100,
  )}%`;
  coalesce("smoothing", {
    type: "setSmoothingStrength",
    strength: Number(elements.smoothing.value),
  });
});
elements.color.addEventListener("pointerdown", () => {
  if (state?.layers.selection.type === "background") {
    backgroundEditStart = state.layers.backgroundColor
      .slice(0, 3)
      .map((value) => Math.round(value * 255));
  }
});
elements.color.addEventListener("input", () => {
  const color = hexToRgba(elements.color.value);
  if (state?.layers.selection.type === "background") {
    coalesce("background-color", {
      type: "setBackgroundColor",
      color: color.slice(0, 3),
    });
  } else {
    coalesce("brush-color", { type: "setBrushColor", color });
  }
});
elements.color.addEventListener("change", () => {
  if (backgroundEditStart) {
    void dispatch({
      type: "commitBackgroundColor",
      before: backgroundEditStart,
      after: hexToRgba(elements.color.value).slice(0, 3),
    });
    backgroundEditStart = undefined;
  }
});
elements.undo.addEventListener("click", () => void dispatch({ type: "undo" }));
elements.redo.addEventListener("click", () => void dispatch({ type: "redo" }));
elements.fit.addEventListener("click", () => void dispatch({ type: "fitCanvas" }));
elements.addLayer.addEventListener("click", () => void dispatch({ type: "addLayer" }));
elements.deleteLayer.addEventListener("click", () =>
  void dispatch({ type: "deleteSelectedLayer" }),
);
elements.saveSettings.addEventListener("click", () =>
  void dispatch({ type: "saveSettings" }),
);
elements.reloadSettings.addEventListener("click", () =>
  void dispatch({ type: "reloadConfiguration" }),
);
elements.resetBrush.addEventListener("click", () =>
  void dispatch({ type: "resetBrush" }),
);

if (listen) {
  await listen("ui-state", (event) => render(event.payload));
  await dispatch({ type: "requestSnapshot" });
} else {
  showLocalError("Controls must run inside Chromazen");
}
