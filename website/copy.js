// "copy" buttons: copy the panel's text without the "$ " prompts or # comments.
for (const button of document.querySelectorAll("button.copy")) {
  button.addEventListener("click", async () => {
    const panel = button.closest(".panel");
    const text = [...panel.querySelectorAll(".line")]
      .map((line) => {
        const clone = line.cloneNode(true);
        for (const skip of clone.querySelectorAll(".p, .c")) skip.remove();
        return clone.textContent.trim();
      })
      .filter(Boolean)
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      button.textContent = "copied";
    } catch {
      button.textContent = "select + copy";
    }
    setTimeout(() => (button.textContent = "copy"), 1600);
  });
}
