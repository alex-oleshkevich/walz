(function () {
  "use strict";

  let dndEnabled = false;
  let replayingClipboardPaste = false;

  // ============================================
  // NOTIFICATION INTERCEPTION
  // ============================================
  const OriginalNotification = window.Notification;

  window.Notification = new Proxy(OriginalNotification, {
    construct(_target, args) {
      const [title, options = {}] = args;
      if (window.__TAURI__ && !dndEnabled) {
        window.__TAURI__.core
          .invoke("send_notification", {
            title: title || "Walz",
            body: options.body || "",
            chatId: options.tag || null,
          })
          .catch(() => {});
      }
      return { close() {} };
    },
    get(target, prop) {
      return target[prop];
    },
  });

  if ("serviceWorker" in navigator) {
    ServiceWorkerRegistration.prototype.showNotification = function (title, options) {
      if (window.__TAURI__ && !dndEnabled) {
        window.__TAURI__.core
          .invoke("send_notification", {
            title: title || "Walz",
            body: options?.body || "",
            chatId: options?.tag || null,
          })
          .catch(() => {});
      }
      return Promise.resolve();
    };
  }

  function openChat(chatId) {
    if (!chatId) return;

    // Extract numeric part from chatId (e.g., "70691051323564@lid" -> "70691051323564")
    const numericId = chatId.replace(/@.*$/, "");

    // Try direct data-id match with various suffixes
    const suffixes = ["@lid", "@c.us", "@s.whatsapp.net", "@g.us", ""];
    for (const suffix of suffixes) {
      const testId = numericId + suffix;
      const el = document.querySelector(`[data-id="${testId}"]`);
      if (el) {
        el.click();
        return;
      }
    }

    // Search all elements with data-id containing the numeric part
    const allWithDataId = document.querySelectorAll('[data-id]');
    for (const el of allWithDataId) {
      const dataId = el.getAttribute("data-id");
      if (dataId && dataId.includes(numericId)) {
        el.click();
        return;
      }
    }

    // Try chat list items
    const chatItems = document.querySelectorAll('[role="listitem"], [data-testid="cell-frame-container"]');
    for (const item of chatItems) {
      const container = item.closest('[data-id]') || item;
      const dataId = container.getAttribute("data-id") || "";
      if (dataId.includes(numericId)) {
        item.click();
        return;
      }
    }
  }

  // ============================================
  // UNREAD COUNT DETECTION
  // ============================================
  function getUnreadCount() {
    const title = document.title;
    const match = title.match(/^\((\d+)\)/);
    return match ? parseInt(match[1], 10) : 0;
  }

  function updateUnreadBadge() {
    const count = getUnreadCount();
    if (window.__TAURI__) {
      window.__TAURI__.core.invoke("update_badge", { count }).catch(() => {});
    }
  }

  function setupTitleObserver() {
    const target = document.querySelector("title") || document.head;
    if (target) {
      const titleObserver = new MutationObserver(updateUnreadBadge);
      titleObserver.observe(target, {
        subtree: true,
        childList: true,
        characterData: true,
      });
    }
    setInterval(updateUnreadBadge, 5000);
  }

  // ============================================
  // THEME SYNC
  // ============================================
  function applyTheme(isDark) {
    document.body.classList.toggle("dark", isDark);
    let style = document.getElementById("tauri-theme");
    if (!style) {
      style = document.createElement("style");
      style.id = "tauri-theme";
      document.head.appendChild(style);
    }
    style.textContent = `:root { color-scheme: ${isDark ? "dark" : "light"} !important; }`;
  }

  async function initTheme() {
    if (window.__TAURI__) {
      try {
        const theme = await window.__TAURI__.core.invoke("get_system_theme");
        applyTheme(theme === "dark");
      } catch {}
    }
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    applyTheme(mq.matches);
    mq.addEventListener("change", (e) => applyTheme(e.matches));
  }

  // ============================================
  // CUSTOM CSS
  // ============================================
  async function loadCustomCSS() {
    if (window.__TAURI__) {
      try {
        const css = await window.__TAURI__.core.invoke("get_custom_css");
        if (css) {
          let customStyle = document.getElementById("custom-css");
          if (!customStyle) {
            customStyle = document.createElement("style");
            customStyle.id = "custom-css";
            document.head.appendChild(customStyle);
          }
          customStyle.textContent = css;
        }
      } catch {}
    }
  }

  // ============================================
  // ZOOM CONTROLS
  // ============================================
  let currentZoom = 1.0;

  function setZoom(level) {
    currentZoom = Math.max(0.5, Math.min(2.0, level));
    document.body.style.zoom = currentZoom;
    if (window.__TAURI__) {
      window.__TAURI__.core.invoke("save_zoom", { zoom: currentZoom }).catch(() => {});
    }
  }

  function clipboardLog(...args) {
    if (window.__WALZ_DEBUG_CLIPBOARD) console.log("[walz clipboard]", ...args);
  }

  // WebKitGTK strips the DataTransfer it hands to web content:
  //   - image on the clipboard -> types is EMPTY, files.length is 0
  //   - file copied in a file manager -> types advertises "text/uri-list",
  //     but getData("text/uri-list") returns ""
  //   - plain text -> exposed normally
  // So files can never be detected from the event itself. Instead we detect the
  // absence of a usable text payload and ask the native side what is really on
  // the clipboard.
  function readTextPayload(clipboardData) {
    const read = (type) => {
      try {
        return clipboardData.getData(type) || "";
      } catch {
        return "";
      }
    };
    return { text: read("text/plain"), html: read("text/html") };
  }

  function shouldUseNativeClipboard(event, payload) {
    const clipboardData = event.clipboardData;
    if (!clipboardData) return false;

    // WebKit decoded the clipboard itself - let WhatsApp handle it natively.
    const hasNativeFiles =
      clipboardData.files?.length > 0 ||
      Array.from(clipboardData.items || []).some((item) => item.kind === "file");
    if (hasNativeFiles) {
      clipboardLog("native files present, not intercepting");
      return false;
    }

    const types = Array.from(clipboardData.types || []);

    // A file manager copy: advertised but unreadable from JS.
    if (types.includes("text/uri-list")) return true;

    // An image: WebKit reports nothing at all.
    if (!payload.text && !payload.html) return true;

    return false;
  }

  function decodeBase64(data) {
    const encoded = atob(data);
    const bytes = new Uint8Array(encoded.length);
    for (let index = 0; index < encoded.length; index++) {
      bytes[index] = encoded.charCodeAt(index);
    }
    return bytes;
  }

  function dispatchPaste(target, dataTransfer) {
    replayingClipboardPaste = true;
    try {
      target.dispatchEvent(new ClipboardEvent("paste", {
        bubbles: true,
        cancelable: true,
        clipboardData: dataTransfer,
      }));
    } finally {
      replayingClipboardPaste = false;
    }
  }

  function dispatchClipboardFiles(target, files) {
    const dataTransfer = new DataTransfer();
    for (const item of files) {
      const file = new File([decodeBase64(item.data)], item.name, { type: item.mime });
      dataTransfer.items.add(file);
    }
    clipboardLog("replaying paste with files", files.map((f) => `${f.name} (${f.mime})`));
    dispatchPaste(target, dataTransfer);
  }

  // Interception is a bet that the clipboard holds files. When it does not (e.g. a
  // URL copied from a browser also advertises text/uri-list), replay the text we
  // captured so the paste is never silently swallowed.
  function dispatchTextFallback(target, payload) {
    if (!payload.text && !payload.html) {
      clipboardLog("no files and no text to replay");
      return;
    }
    const dataTransfer = new DataTransfer();
    if (payload.text) dataTransfer.setData("text/plain", payload.text);
    if (payload.html) dataTransfer.setData("text/html", payload.html);
    clipboardLog("no files on clipboard, replaying text paste");
    dispatchPaste(target, dataTransfer);
  }

  function setupClipboardInterceptor() {
    document.addEventListener("paste", (event) => {
      if (replayingClipboardPaste || !window.__TAURI__) return;

      const payload = event.clipboardData ? readTextPayload(event.clipboardData) : null;
      if (!payload) return;

      clipboardLog("paste event", {
        types: Array.from(event.clipboardData.types || []),
        files: event.clipboardData.files?.length ?? 0,
        text: payload.text.slice(0, 60),
      });

      if (!shouldUseNativeClipboard(event, payload)) return;

      const target = event.target;
      if (!(target instanceof EventTarget)) return;

      event.preventDefault();
      event.stopImmediatePropagation();
      window.__TAURI__.core
        .invoke("get_clipboard_files")
        .then((files) => {
          if (Array.isArray(files) && files.length > 0) {
            dispatchClipboardFiles(target, files);
          } else {
            dispatchTextFallback(target, payload);
          }
        })
        .catch((error) => {
          console.error("[walz clipboard] get_clipboard_files failed:", error);
          dispatchTextFallback(target, payload);
        });
    }, true);
  }

  async function loadZoom() {
    if (window.__TAURI__) {
      try {
        const zoom = await window.__TAURI__.core.invoke("get_zoom");
        if (zoom) setZoom(zoom);
      } catch {}
    }
  }

  // ============================================
  // KEYBOARD SHORTCUTS
  // ============================================
  document.addEventListener("keydown", (e) => {
    // Ctrl/Cmd + F: Focus search
    if ((e.ctrlKey || e.metaKey) && e.key === "f") {
      e.preventDefault();
      const searchBtn = document.querySelector('[data-testid="chat-list-search"]') ||
                        document.querySelector('[title="Search"]') ||
                        document.querySelector('button[aria-label*="Search"]');
      if (searchBtn) searchBtn.click();
      const searchInput = document.querySelector('[data-testid="search-input"]') ||
                          document.querySelector('input[title="Search"]');
      if (searchInput) searchInput.focus();
    }

    // Ctrl/Cmd + Plus: Zoom in
    if ((e.ctrlKey || e.metaKey) && (e.key === "=" || e.key === "+")) {
      e.preventDefault();
      setZoom(currentZoom + 0.1);
    }

    // Ctrl/Cmd + Minus: Zoom out
    if ((e.ctrlKey || e.metaKey) && e.key === "-") {
      e.preventDefault();
      setZoom(currentZoom - 0.1);
    }

    // Ctrl/Cmd + 0: Reset zoom
    if ((e.ctrlKey || e.metaKey) && e.key === "0") {
      e.preventDefault();
      setZoom(1.0);
    }

    // Ctrl/Cmd + N: New chat
    if ((e.ctrlKey || e.metaKey) && e.key === "n") {
      e.preventDefault();
      const newChatBtn = document.querySelector('[data-testid="new-chat-btn"]') ||
                         document.querySelector('[title="New chat"]');
      if (newChatBtn) newChatBtn.click();
    }

    // Escape: Close panels/modals
    if (e.key === "Escape") {
      const closeBtn = document.querySelector('[data-testid="popup-close-btn"]') ||
                       document.querySelector('[aria-label="Close"]');
      if (closeBtn) closeBtn.click();
    }
  });

  // ============================================
  // MPRIS MEDIA CONTROLS
  // ============================================
  function getCurrentAudio() {
    const audios = document.querySelectorAll("audio");
    for (const audio of audios) {
      if (!audio.paused) return audio;
    }
    return audios[0] || null;
  }

  function mprisPlay() {
    const audio = getCurrentAudio();
    if (audio) audio.play();
  }

  function mprisPause() {
    const audio = getCurrentAudio();
    if (audio) audio.pause();
  }

  function mprisPlayPause() {
    const audio = getCurrentAudio();
    if (audio) {
      if (audio.paused) audio.play();
      else audio.pause();
    }
  }

  function mprisStop() {
    const audio = getCurrentAudio();
    if (audio) {
      audio.pause();
      audio.currentTime = 0;
    }
  }

  function mprisSeek(offsetMicros) {
    const audio = getCurrentAudio();
    if (audio) {
      audio.currentTime += offsetMicros / 1000000;
    }
  }

  function mprisSetPosition(positionMicros) {
    const audio = getCurrentAudio();
    if (audio) {
      audio.currentTime = positionMicros / 1000000;
    }
  }

  // ============================================
  // INITIALIZATION
  // ============================================
  function init() {
    initTheme();
    setupTitleObserver();
    loadCustomCSS();
    loadZoom();
  }

  function setupDownloadInterceptor() {
    const origClick = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function () {
      if (this.download) {
        window.__TAURI__.core
          .invoke("set_pending_download_name", { name: this.download })
          .catch(() => {});
      }
      return origClick.apply(this, arguments);
    };
  }

  // Dropped files arrive as a serialized event payload (the old implementation
  // eval'd a generated script with the filename interpolated unescaped). Route
  // them through the same paste replay the clipboard path uses, since that is the
  // route WhatsApp is known to accept.
  function findComposer() {
    const selectors = [
      'footer [contenteditable="true"]',
      '[data-testid="conversation-compose-box-input"]',
      '#main [contenteditable="true"]',
      '[contenteditable="true"]',
    ];
    for (const selector of selectors) {
      const el = document.querySelector(selector);
      if (el) return el;
    }
    return document.body;
  }

  function setupDropListener() {
    window.__TAURI__.event.listen("files-dropped", (event) => {
      const files = event.payload;
      if (!Array.isArray(files) || files.length === 0) return;
      const target = findComposer();
      target.focus?.();
      dispatchClipboardFiles(target, files);
    });
  }

  function setupTauriListeners() {
    setupDownloadInterceptor();
    setupClipboardInterceptor();
    setupDropListener();
    window.__TAURI__.event.listen("system-theme-changed", (e) => {
      applyTheme(e.payload);
    });

    window.__TAURI__.event.listen("set-dnd", (e) => {
      dndEnabled = e.payload;
    });

    window.__TAURI__.event.listen("zoom-in", () => setZoom(currentZoom + 0.1));
    window.__TAURI__.event.listen("zoom-out", () => setZoom(currentZoom - 0.1));
    window.__TAURI__.event.listen("zoom-reset", () => setZoom(1.0));
    window.__TAURI__.event.listen("focus-search", () => {
      const searchBtn = document.querySelector('[data-testid="chat-list-search"]');
      if (searchBtn) searchBtn.click();
    });

    window.__TAURI__.event.listen("notification-clicked", (e) => {
      openChat(e.payload);
    });

    window.__TAURI__.event.listen("mpris-play", mprisPlay);
    window.__TAURI__.event.listen("mpris-pause", mprisPause);
    window.__TAURI__.event.listen("mpris-play-pause", mprisPlayPause);
    window.__TAURI__.event.listen("mpris-stop", mprisStop);
    window.__TAURI__.event.listen("mpris-seek", (e) => mprisSeek(e.payload));
    window.__TAURI__.event.listen("mpris-set-position", (e) => mprisSetPosition(e.payload));
    window.__TAURI__.event.listen("mpris-next", () => {});
    window.__TAURI__.event.listen("mpris-previous", () => {});
  }

  function waitForTauri(callback) {
    if (window.__TAURI__) {
      callback();
    } else {
      let attempts = 0;
      const interval = setInterval(() => {
        attempts++;
        if (window.__TAURI__) {
          clearInterval(interval);
          callback();
        } else if (attempts > 50) {
          clearInterval(interval);
        }
      }, 100);
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => {
      waitForTauri(() => {
        setupTauriListeners();
        init();
      });
    });
  } else {
    waitForTauri(() => {
      setupTauriListeners();
      init();
    });
  }
})();
