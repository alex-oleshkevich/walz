(function () {
  "use strict";

  let dndEnabled = false;
  let replayingClipboardPaste = false;
  let pendingAudioCapture = null;

  const originalMediaPlay = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function (...args) {
    const capture = pendingAudioCapture;
    const url = this.currentSrc || this.src;
    if (capture && url?.startsWith("blob:")) {
      if (capture.url && capture.url !== url) return originalMediaPlay.apply(this, args);
      if (!capture.started) {
        capture.started = true;
        capture.url = url;
        fetch(url)
          .then((response) => {
            if (!response.ok) throw new Error("Could not read the voice message.");
            return response.blob();
          })
          .then(capture.resolve, capture.reject);
      }
      return Promise.resolve();
    }
    return originalMediaPlay.apply(this, args);
  };

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
  // VOICE MESSAGE TRANSCRIPTION
  // ============================================
  const AUDIO_HINTS =
    '[data-icon="ptt-status"], [aria-label="Voice message"], [aria-label="Play voice message"], audio';
  const MAX_AUDIO_BYTES = 25 * 1024 * 1024;
  let menuMessage = null;
  let menuExpiresAt = 0;
  const inlineTranslations = new WeakMap();

  function isAudioMessage(row) {
    return !!row.querySelector(AUDIO_HINTS);
  }

  function transportButton(row) {
    const buttons = [...row.querySelectorAll("button")];
    const slider = row.querySelector('[role="slider"]');
    if (slider) {
      const before = buttons.filter(
        (button) => button.compareDocumentPosition(slider) & Node.DOCUMENT_POSITION_FOLLOWING,
      );
      if (before.length) return before[before.length - 1];
    }
    return buttons.find((button) => !/\d\s*[.,]?\d*\s*[x×]/i.test(button.textContent || ""));
  }

  function isDownloadButton(button) {
    return !!button?.querySelector('[data-icon*="download"]') ||
      /download/i.test(`${button?.getAttribute("aria-label") || ""} ${button?.textContent || ""}`);
  }

  function press(button) {
    const options = { bubbles: true, cancelable: true, composed: true };
    if (window.PointerEvent) {
      button.dispatchEvent(new PointerEvent("pointerdown", options));
      button.dispatchEvent(new PointerEvent("pointerup", options));
    }
    button.dispatchEvent(new MouseEvent("mousedown", options));
    button.dispatchEvent(new MouseEvent("mouseup", options));
    button.click();
  }

  async function waitForPlayButton(row) {
    for (let attempt = 0; attempt < 100; attempt++) {
      const button = transportButton(row);
      if (button && !isDownloadButton(button)) return button;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    throw new Error("WhatsApp could not download this voice message.");
  }

  async function audioBlobFor(row, setStatus) {
    const audio = row.querySelector("audio");
    if (audio?.src?.startsWith("blob:")) {
      try {
        const response = await fetch(audio.src);
        if (response.ok) return response.blob();
      } catch {}
    }

    let button = transportButton(row);
    if (!button) throw new Error("Could not find this message's audio control.");
    if (isDownloadButton(button)) {
      setStatus("Downloading voice message…");
      press(button);
      button = await waitForPlayButton(row);
    }

    setStatus("Reading voice message…");
    let timeout;
    const blob = new Promise((resolve, reject) => {
      timeout = setTimeout(() => reject(new Error("WhatsApp did not prepare the audio.")), 30000);
      pendingAudioCapture = { resolve, reject, started: false };
    });
    try {
      press(button);
      return await blob;
    } finally {
      clearTimeout(timeout);
      // WhatsApp can call play after setting src, so keep playback suppressed
      // briefly after the Blob is captured.
      const capture = pendingAudioCapture;
      setTimeout(() => {
        if (pendingAudioCapture === capture) pendingAudioCapture = null;
      }, 1500);
    }
  }

  function audioFormat(blob) {
    const type = blob.type.toLowerCase();
    const name = blob.name?.toLowerCase() || "";
    const extension = name.split(".").pop();
    if (type.includes("ogg") || type.includes("opus")) return "ogg";
    if (type.includes("mpeg")) return "mp3";
    if (type.includes("wav")) return "wav";
    if (type.includes("webm")) return "webm";
    if (type.includes("mp4")) return "mp4";
    if (type.includes("flac")) return "flac";
    if (["ogg", "opus", "mp3", "wav", "m4a", "mp4", "webm", "flac", "aac"].includes(extension)) {
      return extension === "opus" ? "ogg" : extension;
    }
    return "ogg";
  }

  function audioBase64(blob) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(",", 2)[1]);
      reader.onerror = () => reject(new Error("Could not read the audio file."));
      reader.readAsDataURL(blob);
    });
  }

  function showFormattedText(container, value) {
    container.replaceChildren();
    const blocks = value.trim().split(/\n\s*\n/);
    for (const block of blocks) {
      const sentences = Intl.Segmenter
        ? [...new Intl.Segmenter(undefined, { granularity: "sentence" }).segment(block)]
            .map((part) => part.segment.trim()).filter(Boolean)
        : [block.trim()];
      let paragraph = "";
      for (const sentence of sentences) {
        if (paragraph && paragraph.length + sentence.length > 420) {
          const element = document.createElement("p");
          element.textContent = paragraph;
          container.appendChild(element);
          paragraph = "";
        }
        paragraph += `${paragraph ? " " : ""}${sentence}`;
      }
      if (paragraph) {
        const element = document.createElement("p");
        element.textContent = paragraph;
        container.appendChild(element);
      }
    }
  }

  function messageTextElements(row) {
    return [...row.querySelectorAll('span[data-testid="selectable-text"], span[data-testid$="caption selectable-text"]')].filter(
      (element) => !element.closest('[data-testid="quoted-message"]') &&
        !element.parentElement?.closest('span[data-testid="selectable-text"], span[data-testid$="caption selectable-text"]'),
    );
  }

  function messageText(row) {
    const elements = messageTextElements(row);
    const read = (node) => {
      if (node.nodeType === Node.TEXT_NODE) return node.textContent;
      if (node.nodeName === "BR") return "\n";
      if (node.nodeName === "IMG") return node.getAttribute("data-plain-text") || node.alt || "";
      return [...node.childNodes].map(read).join("");
    };
    return elements.map(read).join("\n").trim();
  }

  async function translateMessageInline(row, text) {
    let state = inlineTranslations.get(row);
    if (state?.translation.isConnected) {
      if (state.pending) return;
      if (state.translated) {
        state.originals.forEach((element) => element.classList.add("walz-original-hidden"));
        state.translation.hidden = false;
        state.toggle.textContent = "Show original";
        return;
      }
    } else {
      const originals = messageTextElements(row);
      if (!originals.length) return;
      const translation = originals[0].cloneNode(false);
      translation.removeAttribute("data-testid");
      translation.classList.add("walz-inline-translation");
      translation.setAttribute("dir", "auto");
      translation.hidden = true;
      originals[0].after(translation);

      const toggle = document.createElement("button");
      toggle.type = "button";
      toggle.className = "walz-inline-toggle";
      toggle.textContent = "Show original";
      toggle.hidden = true;
      translation.after(toggle);

      const status = document.createElement("span");
      status.className = "walz-inline-status";
      status.setAttribute("role", "status");
      toggle.after(status);
      state = { originals, translation, toggle, status, pending: false, translated: false };
      inlineTranslations.set(row, state);

      toggle.addEventListener("click", (event) => {
        event.stopPropagation();
        const showOriginal = toggle.textContent === "Show original";
        originals.forEach((element) => element.classList.toggle("walz-original-hidden", !showOriginal));
        translation.hidden = showOriginal;
        toggle.textContent = showOriginal ? "Show translation" : "Show original";
      });
    }

    state.pending = true;
    state.status.textContent = "Translating…";
    try {
      const result = await window.__TAURI__.core.invoke("translate_transcript", {
        text,
        targetLanguage: "English",
      });
      if (!state.translation.isConnected) return;
      state.translation.textContent = result;
      state.translation.hidden = false;
      state.originals.forEach((element) => element.classList.add("walz-original-hidden"));
      state.toggle.textContent = "Show original";
      state.toggle.hidden = false;
      state.status.remove();
      state.translated = true;
    } catch (error) {
      state.status.textContent = String(error);
    } finally {
      state.pending = false;
    }
  }

  function createTranscriptionDialog(row) {
    document.getElementById("walz-transcription-overlay")?.remove();
    const overlay = document.createElement("div");
    overlay.id = "walz-transcription-overlay";
    overlay.innerHTML = `
      <div class="walz-transcription-dialog" role="dialog" aria-modal="true" aria-labelledby="walz-transcription-title">
        <header><h2 id="walz-transcription-title">Voice message</h2><button class="walz-close" type="button" aria-label="Close"><svg viewBox="0 0 24 24" width="20" height="20" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg></button></header>
        <div class="walz-toolbar" hidden>
          <button class="walz-copy" type="button">Copy</button>
          <div class="walz-translation-controls"><label for="walz-target-language">Translate to</label><input id="walz-target-language" value="English" maxlength="64"><button class="walz-translate" type="button">Translate</button></div>
        </div>
        <p class="walz-status" role="status">Getting audio…</p>
        <div class="walz-result" hidden><div class="walz-transcript"></div><div class="walz-translation-result" hidden><div class="walz-translation-label">Translation</div><div class="walz-translated"></div></div></div>
        <div class="walz-file-fallback" hidden><p>Choose the downloaded audio file to transcribe it.</p><input type="file" accept="audio/*,.ogg,.opus,.m4a,.mp3,.wav,.webm,.flac"></div>
      </div>`;
    document.body.appendChild(overlay);
    const close = () => {
      overlay.remove();
      document.removeEventListener("keydown", onEscape);
    };
    overlay.querySelector(".walz-close").addEventListener("click", close);
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) close();
    });
    const onEscape = (event) => {
      if (event.key === "Escape") {
        close();
      }
    };
    document.addEventListener("keydown", onEscape);
    const status = overlay.querySelector(".walz-status");
    const fallback = overlay.querySelector(".walz-file-fallback");
    const transcript = overlay.querySelector(".walz-transcript");
    const translated = overlay.querySelector(".walz-translated");
    let transcriptText = "";

    async function transcribe(blob) {
      if (!overlay.isConnected) return;
      if (!blob.size || blob.size > MAX_AUDIO_BYTES) {
        throw new Error("Audio must be smaller than 25 MB.");
      }
      status.textContent = "Transcribing…";
      const audioBase64Data = await audioBase64(blob);
      if (!overlay.isConnected) return;
      const text = await window.__TAURI__.core.invoke("transcribe_audio", {
        audioBase64: audioBase64Data,
        format: audioFormat(blob),
      });
      transcriptText = text;
      showFormattedText(transcript, text);
      overlay.querySelector(".walz-result").hidden = false;
      overlay.querySelector(".walz-toolbar").hidden = false;
      status.hidden = true;
    }

    overlay.querySelector(".walz-copy").addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(transcriptText);
        const button = overlay.querySelector(".walz-copy");
        button.textContent = "Copied";
        setTimeout(() => { if (overlay.isConnected) button.textContent = "Copy"; }, 2000);
      } catch {
        status.hidden = false;
        status.textContent = "Could not copy the transcript.";
      }
    });
    overlay.querySelector(".walz-translate").addEventListener("click", async () => {
      const button = overlay.querySelector(".walz-translate");
      button.disabled = true;
      status.hidden = false;
      status.textContent = "Translating…";
      try {
        const text = await window.__TAURI__.core.invoke("translate_transcript", {
          text: transcriptText,
          targetLanguage: overlay.querySelector("#walz-target-language").value,
        });
        showFormattedText(translated, text);
        overlay.querySelector(".walz-translation-result").hidden = false;
        status.hidden = true;
      } catch (error) {
        status.textContent = String(error);
      } finally {
        button.disabled = false;
      }
    });
    fallback.querySelector("input").addEventListener("change", async (event) => {
      const file = event.target.files?.[0];
      if (!file) return;
      fallback.hidden = true;
      try {
        await transcribe(file);
      } catch (error) {
        status.hidden = false;
        status.textContent = String(error);
        fallback.hidden = false;
      }
    });

    (async () => {
      let blob;
      try {
        blob = await audioBlobFor(row, (message) => { status.textContent = message; });
      } catch (error) {
        status.hidden = false;
        status.textContent = String(error);
        fallback.hidden = false;
        return;
      }
      try {
        await transcribe(blob);
      } catch (error) {
        status.hidden = false;
        status.textContent = String(error);
      }
    })();
    overlay.querySelector(".walz-close").focus();
  }

  function addTranscribeMenuItem() {
    if (!menuMessage?.isConnected || Date.now() > menuExpiresAt || !isAudioMessage(menuMessage)) return;
    const items = [...document.querySelectorAll('[role="menuitem"], [role="button"]')];
    const hasLabel = (item, label) =>
      item.innerText?.trim() === label || item.textContent.trim().endsWith(label);
    const download = items.find((item) => {
      if (!hasLabel(item, "Download")) return false;
      let parent = item.parentElement;
      for (let level = 0; parent && level < 4; level++, parent = parent.parentElement) {
        const entries = [...parent.querySelectorAll('[role="menuitem"], [role="button"]')];
        if (entries.some((entry) => hasLabel(entry, "Reply")) &&
            entries.some((entry) => hasLabel(entry, "React"))) return true;
      }
      return false;
    });
    if (!download || download.parentElement.querySelector(".walz-transcribe-menu-item")) return;
    const row = menuMessage;
    const item = download.cloneNode(true);
    item.classList.add("walz-transcribe-menu-item");
    item.setAttribute("aria-label", "Transcribe");
    item.tabIndex = 0;
    const walker = document.createTreeWalker(item, NodeFilter.SHOW_TEXT);
    let replacedLabel = false;
    while (walker.nextNode()) {
      if (walker.currentNode.textContent.trim() === "Download") {
        walker.currentNode.textContent = "Transcribe";
        replacedLabel = true;
        break;
      }
    }
    if (!replacedLabel) item.textContent = "Transcribe";
    const icon = item.querySelector("svg");
    if (icon) {
      icon.innerHTML = '<path d="M7 3h8l4 4v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V5a2 2 0 0 1 2-2Zm8 0v5h4M8 12h8M8 16h8" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>';
    }
    item.addEventListener("click", (event) => {
      event.stopPropagation();
      menuMessage = null;
      document.body.click();
      createTranscriptionDialog(row);
    });
    item.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        item.click();
      }
    });
    download.after(item);
  }

  function addTranslateMenuItem() {
    if (!menuMessage?.isConnected || Date.now() > menuExpiresAt || isAudioMessage(menuMessage)) return;
    const row = menuMessage;
    const text = messageText(row);
    if (!text) return;
    const menu = [...document.querySelectorAll('[role="menu"]')].find((element) => {
      const labels = [...element.querySelectorAll('[role="menuitem"]')].map(
        (item) => item.getAttribute("aria-label"),
      );
      return labels.includes("Reply") && labels.includes("React");
    });
    if (!menu || menu.querySelector(".walz-translate-menu-item")) return;
    const anchor = ["Copy", "Download", "Reply"].map((label) =>
      [...menu.querySelectorAll('[role="menuitem"]')].find(
        (item) => item.getAttribute("aria-label") === label,
      ),
    ).find(Boolean);
    if (!anchor) return;
    const item = anchor.cloneNode(true);
    item.classList.add("walz-translate-menu-item");
    item.setAttribute("aria-label", "Translate");
    item.tabIndex = 0;
    const walker = document.createTreeWalker(item, NodeFilter.SHOW_TEXT);
    while (walker.nextNode()) {
      if (walker.currentNode.textContent.trim() === anchor.getAttribute("aria-label")) {
        walker.currentNode.textContent = "Translate";
        break;
      }
    }
    const icon = item.querySelector("svg");
    if (icon) icon.innerHTML = '<path d="M3 6h12M9 3v3m4 0c-.5 5-3.5 9-8 11m2-8c1.5 3 4 5.5 7 7M14 13h7m-3.5-3-4 11m4-11 4 11" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>';
    item.addEventListener("click", (event) => {
      event.stopPropagation();
      menuMessage = null;
      document.body.click();
      translateMessageInline(row, text);
    });
    item.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        item.click();
      }
    });
    anchor.after(item);
  }

  function addMessageMenuItem() {
    addTranscribeMenuItem();
    addTranslateMenuItem();
  }

  function setupTranscription() {
    const style = document.createElement("style");
    style.textContent = `
      #walz-transcription-overlay{position:fixed;inset:0;z-index:2147483647;display:grid;place-items:center;padding:16px;background:var(--WDS-background-dimmer,rgba(0,0,0,.32))}
      #walz-transcription-overlay [hidden]{display:none!important}
      .walz-transcription-dialog{box-sizing:border-box;display:flex;flex-direction:column;width:min(620px,100%);max-height:calc(100vh - 32px);padding:24px;border-radius:16px;background:var(--WDS-surface-elevated-default,var(--panel-background));color:var(--WDS-content-default,var(--primary));box-shadow:0 8px 32px rgba(0,0,0,.2);font:14px "Roboto Variable",Roboto,"Helvetica Neue",Helvetica,sans-serif}
      .walz-transcription-dialog header{display:flex;align-items:center;justify-content:space-between;gap:16px;margin-bottom:16px}
      .walz-transcription-dialog h2{margin:0;font-size:20px;font-weight:500;line-height:28px}
      .walz-transcription-dialog button,.walz-transcription-dialog input{font:inherit}
      .walz-transcription-dialog button{border:0;cursor:pointer}
      .walz-transcription-dialog button:disabled{opacity:.5;cursor:default}
      .walz-transcription-dialog .walz-close{display:grid;place-items:center;width:32px;height:32px;padding:0;border-radius:50%;background:transparent;color:inherit}
      .walz-transcription-dialog .walz-close:hover,.walz-transcription-dialog .walz-copy:hover{background:var(--WDS-surface-highlight,rgba(255,255,255,.1))}
      .walz-toolbar{display:flex;align-items:center;gap:12px;padding-bottom:16px;border-bottom:1px solid var(--border-deeper,rgba(128,128,128,.2))}
      .walz-copy{height:36px;padding:0 16px;border-radius:18px;background:var(--WDS-surface-elevated-emphasized,var(--panel-input-background));color:inherit;white-space:nowrap}
      .walz-translation-controls{display:flex;align-items:center;gap:8px;margin-left:auto}
      .walz-translation-controls label{color:var(--WDS-content-deemphasized,var(--secondary));white-space:nowrap}
      .walz-translation-controls input{box-sizing:border-box;width:112px;height:36px;padding:0 10px;border:1px solid var(--input-border);border-radius:8px;outline:none;background:var(--WDS-surface-elevated-emphasized,var(--panel-input-background));color:inherit}
      .walz-translation-controls input:focus{border-color:var(--WDS-accent,var(--input-border-active))}
      .walz-translate{height:36px;padding:0 16px;border-radius:18px;background:var(--WDS-accent,#21c063);color:var(--WDS-content-on-accent,#0a0a0a);font-weight:500!important}
      .walz-translate:hover{filter:brightness(1.08)}
      .walz-status{margin:4px 0 16px;color:var(--WDS-content-deemphasized,var(--secondary))}
      .walz-result{min-height:0;overflow:auto;padding:18px 2px 0;line-height:1.55;overflow-wrap:anywhere}
      .walz-result p{margin:0 0 14px;white-space:pre-wrap}
      .walz-translation-result{margin-top:20px;padding-top:18px;border-top:1px solid var(--border-deeper,rgba(128,128,128,.2))}
      .walz-translation-label{margin-bottom:12px;color:var(--WDS-content-deemphasized,var(--secondary));font-size:13px;font-weight:500}
      .walz-file-fallback{padding:12px 0}
      .walz-file-fallback input{max-width:100%}
      .walz-original-hidden,.walz-inline-translation[hidden],.walz-inline-toggle[hidden]{display:none!important}
      .walz-inline-translation{white-space:pre-wrap;overflow-wrap:anywhere}
      .walz-inline-toggle{display:block;margin:4px 0 0;padding:0;border:0;background:none;color:var(--WDS-accent,#21c063);font:500 12px "Roboto Variable",Roboto,"Helvetica Neue",Helvetica,sans-serif;cursor:pointer}
      .walz-inline-toggle:hover{text-decoration:underline}
      .walz-inline-status{display:block;margin-top:4px;color:var(--WDS-content-deemphasized,var(--secondary));font:12px "Roboto Variable",Roboto,"Helvetica Neue",Helvetica,sans-serif;overflow-wrap:anywhere}
      @media(max-width:560px){.walz-toolbar{align-items:stretch;flex-wrap:wrap}.walz-translation-controls{margin-left:0;flex:1}.walz-translation-controls input{min-width:0;flex:1}}
    `;
    document.head.appendChild(style);
    document.addEventListener("pointerdown", (event) => {
      const row = event.target.closest('div[role="row"], div[data-id]');
      if (row) {
        menuMessage = row;
        menuExpiresAt = Date.now() + 30000;
        requestAnimationFrame(addMessageMenuItem);
      } else if (!event.target.closest('[role="menu"], .walz-transcribe-menu-item, .walz-translate-menu-item')) {
        menuMessage = null;
      }
    }, true);
    new MutationObserver(() => {
      if (menuMessage) requestAnimationFrame(addMessageMenuItem);
    }).observe(document.body, { childList: true, subtree: true });
  }

  // ============================================
  // INITIALIZATION
  // ============================================
  function init() {
    initTheme();
    setupTitleObserver();
    loadCustomCSS();
    loadZoom();
    openPendingLink();
    setupTranscription();
  }

  // WhatsApp Web routes /send and /accept itself, so a click-to-chat link is a
  // plain navigation. This reloads the app, which is why it only ever runs for
  // an explicit user action.
  function navigateTo(url) {
    if (typeof url === "string" && url.startsWith("https://web.whatsapp.com/")) {
      window.location.href = url;
    }
  }

  // A link passed on the command line is waiting in Rust before any listener
  // exists, so it has to be pulled rather than pushed.
  function openPendingLink() {
    window.__TAURI__.core
      .invoke("take_pending_link")
      .then((url) => {
        if (url) navigateTo(url);
      })
      .catch(() => {});
  }

  // The composer only exists once a chat is open, and openChat clicks through
  // the chat list, so give the pane a few frames to appear.
  function focusComposer(attempt = 0) {
    const composer = document.querySelector('footer [contenteditable="true"]');
    if (composer) {
      composer.focus();
      return;
    }
    if (attempt < 20) {
      setTimeout(() => focusComposer(attempt + 1), 100);
    }
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

    // The Reply action opens the chat and puts the caret in the composer, so the
    // notification leads straight into typing.
    window.__TAURI__.event.listen("notification-reply", (e) => {
      openChat(e.payload);
      focusComposer();
    });

    // A link handed over by a later `walz whatsapp://...` launch. The page is
    // already up, so navigate right away.
    window.__TAURI__.event.listen("open-url", (e) => {
      navigateTo(e.payload);
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
