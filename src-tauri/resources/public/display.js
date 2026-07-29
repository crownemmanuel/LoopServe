(() => {
  const stage = document.getElementById("stage");
  let currentKey = null;
  let mediaEl = null;

  function assetKey(asset) {
    return `${asset.id}|${asset.url}|${asset.updatedAt || ""}`;
  }

  function showIdle() {
    currentKey = null;
    mediaEl = null;
    stage.innerHTML = '<div id="idle" class="idle">Waiting for live media…</div>';
  }

  function playAsset(asset) {
    if (!asset) {
      showIdle();
      return;
    }

    const key = assetKey(asset);
    if (currentKey === key && mediaEl) {
      if (asset.type === "video" && mediaEl.paused) {
        mediaEl.play().catch(() => {});
      }
      return;
    }

    currentKey = key;
    stage.innerHTML = "";

    if (asset.type === "video") {
      const video = document.createElement("video");
      video.src = asset.url;
      video.autoplay = true;
      video.muted = true;
      video.loop = true;
      video.playsInline = true;
      video.setAttribute("playsinline", "");
      video.setAttribute("webkit-playsinline", "");
      stage.appendChild(video);
      mediaEl = video;
      video.play().catch(() => {});
      return;
    }

    const img = document.createElement("img");
    img.src = asset.url;
    img.alt = asset.name || "Live media";
    stage.appendChild(img);
    mediaEl = img;
  }

  async function fetchLive() {
    try {
      const res = await fetch("/api/live", { cache: "no-store" });
      if (!res.ok) return;
      const data = await res.json();
      playAsset(data.live);
    } catch {
      // keep current media if network blips
    }
  }

  function connectEvents() {
    const es = new EventSource("/api/events");

    es.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        playAsset(data.live);
      } catch {
        // ignore malformed events
      }
    };

    es.onerror = () => {
      es.close();
      setTimeout(connectEvents, 2000);
    };
  }

  fetchLive();
  connectEvents();
  setInterval(fetchLive, 5000);
})();
