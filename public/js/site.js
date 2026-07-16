"use strict";

document.addEventListener("click", async (event) => {
  const button = event.target.closest(".copy-btn");
  if (!button) return;

  const block = button.closest(".code-block");
  const code = block?.querySelector("code");
  if (!code || !navigator.clipboard) return;

  try {
    await navigator.clipboard.writeText(code.textContent);
    const toast = document.createElement("div");
    toast.className = "copy-toast";
    toast.textContent = "Copied!";
    block.appendChild(toast);
    window.setTimeout(() => toast.remove(), 1500);
  } catch {
    // Clipboard access can be denied by browser permissions.
  }
});
