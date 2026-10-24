(() => {
  const stickyTop = 64;

  function bindThreadContext(root = document) {
    const mainTweet = root.querySelector?.("#m") || document.querySelector("#m");
    const contextBar = root.querySelector?.(".thread-context-bar") || document.querySelector(".thread-context-bar");
    if (!mainTweet || !contextBar || contextBar.dataset.observed) return;
    const mainThread = mainTweet.closest(".main-thread") || mainTweet;

    contextBar.dataset.observed = "true";

    const update = () => {
      // A conversation can include author follow-ups beneath #m. Keep the
      // compact context hidden until that entire opening thread has passed.
      const mainBottom = mainThread.getBoundingClientRect().bottom;
      contextBar.classList.toggle("is-visible", mainBottom <= stickyTop);
    };

    const observer = new IntersectionObserver(update, {
      rootMargin: `-${stickyTop}px 0px 0px 0px`,
      threshold: 0,
    });

    observer.observe(mainThread);
    update();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => bindThreadContext());
  } else {
    bindThreadContext();
  }
})();
