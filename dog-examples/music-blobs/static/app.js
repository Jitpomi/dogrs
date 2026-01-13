// app.js (type="module")
// Modern Music Player JavaScript with Real-Time Audio Visualization (FIXED)

function closeUploadModal() {
  document.getElementById("uploadModal")?.classList.remove("active");
}
window.closeUploadModal = closeUploadModal;

class MusicPlayer {
  constructor() {
    this.musicLibrary = [];

    this.currentAudio = null;
    this.currentObjectUrl = null;
    this.currentTrackId = null;

    this.isPlaying = false;
    this.isPaused = false;

    this.duration = 0; 

    // waveform
    this.waveformCanvases = new Map(); // trackId -> { canvas, ctx, cssW, cssH, dpr }
    this.animationId = null;

    // audio graph
    this.audioContext = null;
    this.analyser = null;
    this.dataArray = null;
    this.sourceNode = null;

    // decode (for static waveforms)
    this.decodeContext = null;

    this.init();
  }

  init() {
    this.setupEventListeners();
    this.loadMusicLibrary();
  }

  setupEventListeners() {
    // Upload button (header)
    document.querySelector(".upload-btn")?.addEventListener("click", () => {
      this.openUploadModal();
    });

    // Upload area
    const uploadArea = document.getElementById("uploadArea");
    const fileInput = document.getElementById("fileInput");

    if (uploadArea && fileInput) {
      uploadArea.addEventListener("click", () => fileInput.click());

      uploadArea.addEventListener("dragover", (e) => {
        e.preventDefault();
        uploadArea.style.borderColor = "var(--accent-primary)";
        uploadArea.style.background = "rgba(0, 212, 170, 0.08)";
      });

      uploadArea.addEventListener("dragleave", (e) => {
        e.preventDefault();
        uploadArea.style.borderColor = "var(--border-color)";
        uploadArea.style.background = "transparent";
      });

      uploadArea.addEventListener("drop", (e) => {
        e.preventDefault();
        uploadArea.style.borderColor = "var(--border-color)";
        uploadArea.style.background = "transparent";
        const files = Array.from(e.dataTransfer.files || []);
        this.handleFileUpload(files);
      });

      fileInput.addEventListener("change", (e) => {
        const files = Array.from(e.target.files || []);
        this.handleFileUpload(files);
      });
    }

    // Search functionality (FIX: no .track-meta)
    document.querySelector(".search-input")?.addEventListener("input", (e) => {
      this.filterTracks(e.target.value);
    });

    // Sort selector (optional stub)
    document.querySelector(".sort-select")?.addEventListener("change", (e) => {
      this.sortTracks(e.target.value);
    });

    // View toggle
    document.querySelectorAll(".view-btn").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        document.querySelectorAll(".view-btn").forEach((b) => b.classList.remove("active"));
        e.target.closest(".view-btn")?.classList.add("active");
        this.toggleView(e.target.closest(".view-btn")?.dataset.view);
      });
    });

    // Player controls
    document.getElementById("playPauseBtn")?.addEventListener("click", () => this.togglePlayPause());
    document.getElementById("prevBtn")?.addEventListener("click", () => this.previousTrack());
    document.getElementById("nextBtn")?.addEventListener("click", () => this.nextTrack());
    document.getElementById("downloadBtn")?.addEventListener("click", () => this.downloadCurrentTrack());
    document.getElementById("favoriteBtn")?.addEventListener("click", () => this.toggleFavorite());

    // Progress bar seek
    document.querySelector(".progress-bar")?.addEventListener("click", (e) => this.seekTo(e));
  }

  // -----------------------------
  // Library / Rendering
  // -----------------------------
  async loadMusicLibrary() {
    try {
      this.showStatus("Loading music library...", "info");

      const response = await fetch("/music", {
        method: "GET",
        headers: { "Content-Type": "application/json" },
      });

      if (!response.ok) throw new Error(`Failed to load library: ${response.status}`);

      const data = await response.json();
      this.musicLibrary = Array.isArray(data) ? data : [];

      this.renderTracks();
      this.showStatus(`Loaded ${this.musicLibrary.length} tracks`, "success");
    } catch (error) {
      console.error(error);
      this.musicLibrary = [];
      this.renderTracks();
      this.showStatus("Failed to load music library", "error");
    }
  }

  renderTracks() {
    const trackList = document.getElementById("trackList");
    if (!trackList) return;

    if (this.musicLibrary.length === 0) {
      trackList.innerHTML = `
        <div class="empty-state">
          <i class="fas fa-music"></i>
          <h3>No music files found</h3>
          <p>Upload some music files to get started</p>
          <button class="upload-btn" onclick="musicPlayer.openUploadModal()">
            <i class="fas fa-upload"></i>
            Upload Music
          </button>
        </div>`;
      return;
    }

    trackList.innerHTML = this.musicLibrary
      .map((track, index) => {
        const trackId = track.key;
        const title = track.metadata?.title || track.filename || "Unknown Title";
        const artist = track.metadata?.artist || "Unknown Artist";
        const duration = this.formatDuration(track.metadata?.duration || 0);
        const genre = track.metadata?.genre || "Unknown";

        return `
          <div class="track-item ${this.currentTrackId === trackId ? "playing" : ""}" data-track-id="${trackId}">
            <div class="track-index"><span class="index-number">${index + 1}</span></div>

            <div class="track-play-btn">
              <button class="play-button play-btn" data-track-id="${trackId}" style="display:inline-block;">
                <i class="fas fa-play"></i>
              </button>
              <button class="play-button pause-btn" data-track-id="${trackId}" style="display:none;">
                <i class="fas fa-pause"></i>
              </button>
              <button class="play-button stop-btn" data-track-id="${trackId}" style="display:none;">
                <i class="fas fa-stop"></i>
              </button>
            </div>

            <div class="track-artwork">
              ${
                track.metadata?.album_art_url
                  ? `<img src="${track.metadata.album_art_url}" alt="Album Art">` 
                  : `<div class="placeholder"><i class="fas fa-music"></i></div>` 
              }
            </div>

            <div class="track-info">
              <div class="track-title">${title} <span class="track-badge">NEW</span></div>
              <div class="track-artist">${artist}</div>
            </div>


            <div class="track-genre-tag"><span class="genre-badge">${genre}</span></div>
            <div class="track-duration">${duration}</div>
            <div class="track-vocal-indicator"><i class="fas fa-microphone"></i></div>

            <div class="track-waveform">
              ${this.createWaveformContainer(trackId)}
            </div>

            <div class="track-actions">
              <button class="action-btn favorite-btn"><i class="far fa-heart"></i></button>
              <button class="action-btn star-btn"><i class="fas fa-star"></i></button>
              <div class="track-menu-container">
                <button class="action-btn menu-btn" data-track-id="${trackId}">
                  <i class="fas fa-ellipsis-v"></i>
                </button>
              </div>
            </div>
          </div>`;
      })
      .join("");

    // Bind buttons
    document.querySelectorAll(".play-btn").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        this.playTrack(btn.dataset.trackId);
      });
    });

    document.querySelectorAll(".pause-btn").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        this.pauseTrack(btn.dataset.trackId);
      });
    });

    document.querySelectorAll(".stop-btn").forEach((btn) => {
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        this.stopTrack(btn.dataset.trackId);
      });
    });

    // Menu dropdown
    document.querySelectorAll(".menu-btn").forEach((button) => {
      button.addEventListener("click", (e) => {
        e.stopPropagation();
        const trackId = button.dataset.trackId;



        const menu = document.createElement("div");
        menu.innerHTML = `
          <div class="dropdown-item download-item" data-track-id="${trackId}">
            <i class="fas fa-download"></i><span>Download</span>
          </div>
          <div class="dropdown-item delete-item" data-track-id="${trackId}">
            <i class="fas fa-trash"></i><span>Delete</span>
          </div>`;

        const rect = button.getBoundingClientRect();
        menu.style.position = "fixed";
        menu.style.top = `${rect.bottom + 5}px`;
        menu.style.left = `${rect.right - 140}px`;
        menu.style.zIndex = "2147483647";

        document.body.appendChild(menu);

        menu.querySelector(".download-item").addEventListener("click", (ev) => {
          ev.stopPropagation();
          const trackItem = document.querySelector(`[data-track-id="${trackId}"]`);
          const trackTitle = trackItem?.querySelector(".track-title")?.textContent?.replace(" NEW", "").trim() || "track";
          this.downloadTrack(trackId, trackTitle);
          menu.remove();
        });

        menu.querySelector(".delete-item").addEventListener("click", (ev) => {
          ev.stopPropagation();
          const trackItem = document.querySelector(`[data-track-id="${trackId}"]`);
          const trackTitle = trackItem?.querySelector(".track-title")?.textContent?.replace(" NEW", "").trim() || "track";
          this.deleteTrack(trackId, trackTitle);
          menu.remove();
        });
      });
    });

    // Initialize static waveforms with real peaks from backend
    this.musicLibrary.forEach((t) => this.loadWaveformPeaks(t.key));
  }

  createWaveformContainer(trackId) {
    // Use trackId safely in id
    const safe = this.safeId(trackId);
    return `<canvas class="track-waveform-canvas" id="waveform-${safe}"></canvas>`;
  }

  safeId(trackId) {
    // IDs cannot contain lots of characters (slashes, spaces, etc.)
    return String(trackId).replace(/[^a-zA-Z0-9_-]/g, "_");
  }

  ensureWaveformCanvas(trackId) {
    const safe = this.safeId(trackId);
    const canvas = document.getElementById(`waveform-${safe}`);
    if (!canvas) return null;

    // Make canvas match CSS size + DPR correctly
    const cssW = canvas.clientWidth || 320;
    const cssH = canvas.clientHeight || 40;
    const dpr = window.devicePixelRatio || 1;

    // Set backing store size
    canvas.width = Math.floor(cssW * dpr);
    canvas.height = Math.floor(cssH * dpr);

    const ctx = canvas.getContext("2d");
    if (!ctx) return null;

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0); // IMPORTANT: reset + scale once
    this.waveformCanvases.set(trackId, { canvas, ctx, cssW, cssH, dpr });

    // click-to-seek (only for current track)
    canvas.addEventListener("click", (e) => {
      if (this.currentTrackId === trackId && this.currentAudio && this.currentAudio.duration) {
        const rect = canvas.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const pct = Math.max(0, Math.min(1, x / rect.width));
        this.currentAudio.currentTime = pct * this.currentAudio.duration;
      }
    });

    this.drawStaticWaveform(trackId);
    return this.waveformCanvases.get(trackId);
  }

  async loadWaveformPeaks(trackId) {
    try {
      const response = await fetch("/music", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "x-service-method": "peaks",
        },
        body: JSON.stringify({ key: trackId }),
      });

      if (!response.ok) {
        console.warn(`Failed to load peaks for ${trackId}: ${response.status}`);
        // Fall back to static waveform
        this.ensureWaveformCanvas(trackId);
        return;
      }

      const peaksData = await response.json();
      if (peaksData.status === "success" && peaksData.peaks) {
        // Store peaks data for this track
        const wf = this.ensureWaveformCanvas(trackId);
        if (wf) {
          wf.peaks = peaksData.peaks;
          wf.duration = peaksData.duration || 0;
          this.drawRealWaveform(trackId, peaksData.peaks);
        }
      } else {
        // Fall back to static waveform
        this.ensureWaveformCanvas(trackId);
      }
    } catch (error) {
      console.warn(`Error loading peaks for ${trackId}:`, error);
      // Fall back to static waveform
      this.ensureWaveformCanvas(trackId);
    }
  }

  drawRealWaveform(trackId, peaks) {
    const w = this.waveformCanvases.get(trackId);
    if (!w || !peaks || peaks.length === 0) return;

    const { ctx, cssW: width, cssH: height } = w;

    // Clear canvas with proper bounds
    ctx.clearRect(0, 0, width, height);

    // Optimize bar width for 2000 peaks
    const barW = Math.max(0.3, width / peaks.length);
    const gap = Math.min(0.1, barW * 0.1);
    const maxH = height * 0.85;

    for (let i = 0; i < peaks.length; i++) {
      const x = i * (barW + gap);
      if (x >= width) break; // Don't draw beyond canvas
      
      const amp = Math.max(0.01, Math.min(1.0, peaks[i]));
      const h = Math.max(1, amp * maxH);
      const y = (height - h) / 2;

      // Enhanced gradient with better contrast
      const alpha = 0.3 + amp * 0.6;
      const grad = ctx.createLinearGradient(x, y, x, y + h);
      
      if (amp > 0.7) {
        // High amplitude - warmer colors
        grad.addColorStop(0, `rgba(200,210,220,${alpha})`);
        grad.addColorStop(0.5, `rgba(170,180,190,${alpha * 0.9})`);
        grad.addColorStop(1, `rgba(140,150,160,${alpha * 0.8})`);
      } else if (amp > 0.3) {
        // Medium amplitude - neutral colors
        grad.addColorStop(0, `rgba(180,190,200,${alpha})`);
        grad.addColorStop(0.5, `rgba(150,160,170,${alpha * 0.9})`);
        grad.addColorStop(1, `rgba(120,130,140,${alpha * 0.8})`);
      } else {
        // Low amplitude - cooler colors
        grad.addColorStop(0, `rgba(160,170,180,${alpha})`);
        grad.addColorStop(0.5, `rgba(130,140,150,${alpha * 0.9})`);
        grad.addColorStop(1, `rgba(100,110,120,${alpha * 0.8})`);
      }

      ctx.fillStyle = grad;
      ctx.fillRect(x, y, barW, h);

      // Enhanced highlights for peaks
      if (amp > 0.8) {
        const highlightH = Math.max(1, h * 0.15);
        ctx.fillStyle = `rgba(220,230,240,${amp * 0.3})`;
        ctx.fillRect(x, y, barW, highlightH);
        ctx.fillRect(x, y + h - highlightH, barW, highlightH);
      }
    }
  }

  drawStaticWaveform(trackId) {
    const w = this.waveformCanvases.get(trackId);
    if (!w) return;

    const { ctx, cssW: width, cssH: height } = w;

    // Clear in CSS pixels (transform already set)
    ctx.clearRect(0, 0, width, height);

    const barW = 0.7;
    const gap = 0.25;
    const step = barW + gap;
    const count = Math.floor(width / step);

    for (let i = 0; i < count; i++) {
      const x = i * step;
      const p = i / count;

      const bass = Math.sin(p * Math.PI * 4) * 0.30;
      const mid = Math.sin(p * Math.PI * 12) * 0.18;
      const high = Math.sin(p * Math.PI * 32) * 0.12;
      const noise = (Math.random() - 0.5) * 0.08;

      let amp = Math.abs(bass + mid + high + noise);
      const structure = Math.sin(p * Math.PI * 2) * 0.2 + 0.85;
      amp *= structure;

      amp = Math.max(0.03, Math.min(0.9, amp));

      const maxH = height * 0.9;
      const h = amp * maxH;
      const y = (height - h) / 2;

      const a = 0.55 + amp * 0.4;
      const grad = ctx.createLinearGradient(x, y, x, y + h);
      grad.addColorStop(0, `rgba(200,210,220,${a})`);
      grad.addColorStop(0.5, `rgba(170,180,190,${a * 0.92})`);
      grad.addColorStop(1, `rgba(140,150,160,${a * 0.85})`);

      ctx.fillStyle = grad;
      ctx.fillRect(x, y, barW, h);

      if (amp > 0.45) {
        const d = Math.max(1, h * 0.12);
        ctx.fillStyle = `rgba(220,230,240,${amp * 0.55})`;
        ctx.fillRect(x, y, barW, d);
        ctx.fillRect(x, y + h - d, barW, d);
      }
    }
  }

  // -----------------------------
  // Playback
  // -----------------------------
  async playTrack(trackId) {
    try {
      // Resume if same track paused
      if (this.currentTrackId === trackId && this.isPaused && this.currentAudio) {
        await this.currentAudio.play();
        this.isPlaying = true;
        this.isPaused = false;
        this.updatePlayButton(trackId, true);
        this.updatePlayerControls();
        this.startWaveformAnimation(trackId);
        this.showStatus("▶️ Resumed playback", "success");
        return;
      }

      // Stop previous
      if (this.currentAudio && this.currentTrackId && this.currentTrackId !== trackId) {
        await this.stopTrack(this.currentTrackId);
      }

      // Stream from backend
      const streamResponse = await fetch("/music", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "x-service-method": "stream",
        },
        body: JSON.stringify({ key: trackId }),
      });

      if (!streamResponse.ok) throw new Error(`Failed to start stream: ${streamResponse.status}`);

      const streamInfo = await streamResponse.json();
      if (!streamInfo.audio_data) throw new Error("No audio data available in stream response");

      const track = this.musicLibrary.find((t) => t.key === trackId);

      // Make audio element
      this.currentAudio = new Audio();
      this.currentTrackId = trackId;

      // Convert base64 -> blob URL
      const bytes = this.base64ToBytes(streamInfo.audio_data);
      const blob = new Blob([bytes], { type: streamInfo.content_type || "audio/mpeg" });

      // Cleanup old url
      if (this.currentObjectUrl) URL.revokeObjectURL(this.currentObjectUrl);
      this.currentObjectUrl = URL.createObjectURL(blob);
      this.currentAudio.src = this.currentObjectUrl;

      this.setupAudioEventListeners(trackId);

      // Ensure waveform canvas exists
      this.ensureWaveformCanvas(trackId);

      // Setup analyzer (CLEAN graph)
      await this.setupAudioAnalyzer(this.currentAudio);

      await this.currentAudio.play();

      if (track) this.showAudioPlayer(track);
    } catch (error) {
      console.error(error);
      this.showStatus(`❌ Failed to play track: ${error.message}`, "error");
      this.updatePlayButton(trackId, false);
    }
  }

  setupAudioEventListeners(trackId) {
    const a = this.currentAudio;

    a.addEventListener("loadstart", () => this.showStatus("🎵 Loading track...", "info"));

    a.addEventListener("play", () => {
      this.isPlaying = true;
      this.isPaused = false;
      this.updatePlayButton(trackId, true);
      this.updatePlayerControls();
      this.startWaveformAnimation(trackId);
    });

    a.addEventListener("pause", () => {
      // pause can also happen during stop; we'll handle state there too
      this.isPlaying = false;
      this.updatePlayButton(trackId, false);
      this.updatePlayerControls();
      this.stopWaveformAnimation();
    });

    a.addEventListener("ended", () => {
      this.isPlaying = false;
      this.isPaused = false;
      this.updatePlayButton(trackId, false);
      this.updatePlayerControls();
      this.hideAudioPlayer();
      this.cleanupAudio();
      this.showStatus("🎵 Track finished", "info");
    });

    a.addEventListener("timeupdate", () => this.updateProgress());

    a.addEventListener("loadedmetadata", () => {
      this.duration = a.duration || 0;
      this.updateProgress();
    });

    a.addEventListener("error", () => {
      this.showStatus("❌ Audio error", "error");
      this.updatePlayButton(trackId, false);
      this.stopWaveformAnimation();
    });
  }

  async togglePlayPause() {
    if (!this.currentAudio || !this.currentTrackId) return;

    try {
      if (this.isPlaying) {
        await this.pauseTrack(this.currentTrackId);
      } else if (this.isPaused) {
        await this.resumeTrack(this.currentTrackId);
      } else {
        await this.currentAudio.play();
      }
    } catch (e) {
      this.showStatus(`❌ Playback error: ${e.message}`, "error");
    }
  }

  async pauseTrack(trackId) {
    try {
      // backend pause (optional)
      await fetch("/music", {
        method: "POST",
        headers: { "Content-Type": "application/json", "x-service-method": "pause" },
        body: JSON.stringify({ key: trackId }),
      });

      if (this.currentAudio) {
        this.currentAudio.pause();
        this.isPaused = true;
        this.isPlaying = false;
        this.updatePlayButton(trackId, false);
        this.updatePlayerControls();
        this.showStatus("⏸️ Paused", "info");
      }
    } catch (e) {
      this.showStatus(`❌ Pause failed: ${e.message}`, "error");
    }
  }

  async resumeTrack(trackId) {
    try {
      // backend resume (optional)
      await fetch("/music", {
        method: "POST",
        headers: { "Content-Type": "application/json", "x-service-method": "resume" },
        body: JSON.stringify({ key: trackId }),
      });

      if (this.currentAudio) {
        await this.currentAudio.play();
        this.isPaused = false;
        this.isPlaying = true;
        this.updatePlayButton(trackId, true);
        this.updatePlayerControls();
        this.startWaveformAnimation(trackId);
        this.showStatus("▶️ Resumed", "success");
      }
    } catch (e) {
      this.showStatus(`❌ Resume failed: ${e.message}`, "error");
    }
  }

  async stopTrack(trackId) {
    try {
      // backend cancel (optional)
      await fetch("/music", {
        method: "POST",
        headers: { "Content-Type": "application/json", "x-service-method": "cancel" },
        body: JSON.stringify({ key: trackId }),
      });

      // frontend stop
      this.stopWaveformAnimation();
      this.cleanupAudio();

      this.currentTrackId = null;
      this.isPlaying = false;
      this.isPaused = false;

      this.updatePlayButton(trackId, false);
      this.updatePlayerControls();
      this.hideAudioPlayer();
      this.showStatus("⏹️ Stopped", "info");
    } catch (e) {
      this.showStatus(`❌ Stop failed: ${e.message}`, "error");
    }
  }

  cleanupAudio() {
    // First teardown audio graph before manipulating audio element
    this.teardownAudioGraph();
    
    if (this.currentAudio) {
      try {
        this.currentAudio.pause();
        this.currentAudio.currentTime = 0;
      } catch (e) {
        console.warn("Error pausing audio:", e);
      }
      
      try {
        this.currentAudio.src = "";
        this.currentAudio.load();
      } catch (e) {
        console.warn("Error clearing audio src:", e);
      }
      
      this.currentAudio = null;
    }
    
    if (this.currentObjectUrl) {
      try {
        URL.revokeObjectURL(this.currentObjectUrl);
      } catch (e) {
        console.warn("Error revoking object URL:", e);
      }
      this.currentObjectUrl = null;
    }
  }

  // -----------------------------
  // Audio Graph (FIXED)
  // -----------------------------
  async setupAudioAnalyzer(audioElement) {
    try {
      // Must be resumed by user gesture in some browsers
      if (!this.audioContext) {
        this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
      }
      if (this.audioContext.state === "suspended") {
        await this.audioContext.resume();
      }

      // teardown previous nodes
      this.teardownAudioGraph();

      this.analyser = this.audioContext.createAnalyser();
      this.analyser.fftSize = 2048;
      this.analyser.smoothingTimeConstant = 0.3;

      const bufferLength = this.analyser.frequencyBinCount;
      this.dataArray = new Uint8Array(bufferLength);

      // connect
      this.sourceNode = this.audioContext.createMediaElementSource(audioElement);
      this.sourceNode.connect(this.analyser);
      this.analyser.connect(this.audioContext.destination);

      return true;
    } catch (e) {
      console.warn("Analyzer setup failed:", e);
      return false;
    }
  }

  teardownAudioGraph() {
    try {
      if (this.sourceNode) {
        this.sourceNode.disconnect();
        this.sourceNode = null;
      }
    } catch (e) {
      console.warn("Error disconnecting source node:", e);
    }
    
    try {
      if (this.analyser) {
        this.analyser.disconnect();
        this.analyser = null;
      }
    } catch (e) {
      console.warn("Error disconnecting analyser:", e);
    }
    
    this.dataArray = null;
  }

  // -----------------------------
  // Waveform animation (FIXED scaling)
  // -----------------------------
  startWaveformAnimation(trackId) {
    this.stopWaveformAnimation();

    const wf = this.waveformCanvases.get(trackId);
    if (!wf || !this.analyser || !this.dataArray) return;

    const { ctx, cssW: width, cssH: height, dpr } = wf;

    // Ensure transform is correct every time we start
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const animate = () => {
      if (this.currentTrackId !== trackId || !this.isPlaying || !this.analyser) return;

      this.analyser.getByteFrequencyData(this.dataArray);

      // If we have real peaks data, use it as the base and overlay live frequency data
      if (wf.peaks && wf.peaks.length > 0) {
        this.drawRealWaveform(trackId, wf.peaks);
        
        // Overlay live frequency visualization
        const barW = width / wf.peaks.length;
        const maxH = height * 0.9;
        
        for (let i = 0; i < Math.min(wf.peaks.length, this.dataArray.length); i++) {
          const x = i * barW;
          const staticAmp = wf.peaks[i];
          const liveFreq = this.dataArray[Math.floor((i / wf.peaks.length) * this.dataArray.length)] / 255;
          
          // Combine static waveform with live frequency data
          const combinedAmp = Math.max(staticAmp, liveFreq * 0.3);
          const h = combinedAmp * maxH;
          const y = (height - h) / 2;
          
          // Add live frequency overlay with accent color
          if (liveFreq > 0.1) {
            const liveH = liveFreq * maxH * 0.4;
            const liveY = (height - liveH) / 2;
            ctx.fillStyle = `rgba(0,212,170,${liveFreq * 0.3})`;
            ctx.fillRect(x, liveY, Math.max(0.5, barW - 0.1), liveH);
          }
        }
      } else {
        // Fall back to original frequency-only visualization
        ctx.clearRect(0, 0, width, height);
        ctx.fillStyle = "rgba(255,255,255,0.02)";
        ctx.fillRect(0, 0, width, height);

        const barW = 0.9;
        const gap = 0.25;
        const step = barW + gap;
        const count = Math.min(Math.floor(width / step), this.dataArray.length);

        for (let i = 0; i < count; i++) {
          const x = i * step;
          const idx = Math.floor((i / count) * this.dataArray.length);
          const f = this.dataArray[idx] / 255;

          const maxH = height * 0.82;
          const h = Math.max(0.8, f * maxH);
          const y = (height - h) / 2;

          const a = 0.25 + f * 0.7;
          const grad = ctx.createLinearGradient(x, y, x, y + h);
          grad.addColorStop(0, `rgba(190,200,210,${a})`);
          grad.addColorStop(0.5, `rgba(165,175,185,${a * 0.92})`);
          grad.addColorStop(1, `rgba(140,150,160,${a * 0.82})`);

          ctx.fillStyle = grad;
          ctx.fillRect(x, y, barW, h);

          if (f > 0.65) {
            ctx.fillStyle = `rgba(220,230,240,${f * 0.35})`;
            ctx.fillRect(x, y, barW, Math.max(1, h * 0.28));
          }
        }
      }

      // Enhanced progress line with better visibility
      if (this.currentAudio?.duration) {
        const p = this.currentAudio.currentTime / this.currentAudio.duration;
        const px = p * width;
        
        // Progress fill
        ctx.fillStyle = "rgba(0,212,170,0.08)";
        ctx.fillRect(0, 0, px, height);
        
        // Progress line
        ctx.fillStyle = "#00d4aa";
        ctx.fillRect(px - 1, 0, 2, height);
        
        // Progress indicator dot
        ctx.beginPath();
        ctx.arc(px, height / 2, 3, 0, 2 * Math.PI);
        ctx.fillStyle = "#00d4aa";
        ctx.fill();
      }

      this.animationId = requestAnimationFrame(animate);
    };

    animate();
  }

  stopWaveformAnimation() {
    if (this.animationId) cancelAnimationFrame(this.animationId);
    this.animationId = null;
  }

  // -----------------------------
  // UI updates
  // -----------------------------
  updatePlayButton(trackId, playing) {
    const trackItem = document.querySelector(`[data-track-id="${CSS.escape(trackId)}"]`);
    if (!trackItem) return;

    const playBtn = trackItem.querySelector(".play-btn");
    const pauseBtn = trackItem.querySelector(".pause-btn");
    const stopBtn = trackItem.querySelector(".stop-btn");

    // Reset other tracks
    document.querySelectorAll(".track-item").forEach((item) => item.classList.remove("playing"));

    if (playing) {
      if (playBtn) playBtn.style.display = "none";
      if (pauseBtn) pauseBtn.style.display = "none";
      if (stopBtn) stopBtn.style.display = "inline-block";
      trackItem.classList.add("playing");
    } else {
      if (playBtn) playBtn.style.display = "inline-block";
      if (pauseBtn) pauseBtn.style.display = "none";
      if (stopBtn) stopBtn.style.display = "none";
    }
  }

  showAudioPlayer(track) {
    const player = document.getElementById("audioPlayer");
    const artwork = document.getElementById("playerArtwork");
    const title = document.getElementById("playerTitle");
    const artist = document.getElementById("playerArtist");

    if (!player) return;

    if (artwork) {
      artwork.src = track.metadata?.album_art_url || "";
      artwork.style.display = track.metadata?.album_art_url ? "block" : "none";
    }
    if (title) title.textContent = track.metadata?.title || track.filename || "Unknown Title";
    if (artist) artist.textContent = track.metadata?.artist || "Unknown Artist";

    player.style.display = "grid";
    this.updatePlayerControls();
  }

  hideAudioPlayer() {
    const p = document.getElementById("audioPlayer");
    if (p) p.style.display = "none";
  }

  updatePlayerControls() {
    const playPauseBtn = document.getElementById("playPauseBtn");
    const icon = playPauseBtn?.querySelector("i");
    if (!icon) return;
    icon.className = this.isPlaying ? "fas fa-pause" : "fas fa-play";
  }

  updateProgress() {
    if (!this.currentAudio) return;

    const currentTime = this.currentAudio.currentTime || 0;
    const duration = this.currentAudio.duration || 0;
    const progress = duration > 0 ? (currentTime / duration) * 100 : 0;

    const fill = document.getElementById("progressFill");
    const handle = document.getElementById("progressHandle");
    const cur = document.getElementById("currentTime");
    const tot = document.getElementById("totalTime");

    if (fill) fill.style.width = `${progress}%`;
    if (handle) handle.style.left = `${progress}%`;
    if (cur) cur.textContent = this.formatDuration(currentTime);
    if (tot) tot.textContent = this.formatDuration(duration);
  }

  seekTo(e) {
    if (!this.currentAudio || !this.currentAudio.duration) return;
    const bar = e.currentTarget;
    const rect = bar.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const pct = Math.max(0, Math.min(1, x / rect.width));
    this.currentAudio.currentTime = pct * this.currentAudio.duration;
  }

  previousTrack() {
    if (!this.currentTrackId) return;
    const i = this.musicLibrary.findIndex((t) => t.key === this.currentTrackId);
    if (i > 0) this.playTrack(this.musicLibrary[i - 1].key);
  }

  nextTrack() {
    if (!this.currentTrackId) return;
    const i = this.musicLibrary.findIndex((t) => t.key === this.currentTrackId);
    if (i >= 0 && i < this.musicLibrary.length - 1) this.playTrack(this.musicLibrary[i + 1].key);
  }

  // -----------------------------
  // Download / Delete
  // -----------------------------
  async downloadTrack(trackId, trackTitle) {
    this.showStatus("📥 Starting download...", "info");

    try {
      const response = await fetch("/music", {
        method: "POST",
        headers: { "Content-Type": "application/json", "x-service-method": "stream" },
        body: JSON.stringify({ key: trackId }),
      });

      if (!response.ok) throw new Error(`Download failed: ${response.status}`);

      const streamInfo = await response.json();
      if (!streamInfo.audio_data) throw new Error("No audio data available for download");

      const bytes = this.base64ToBytes(streamInfo.audio_data);
      const blob = new Blob([bytes], { type: streamInfo.content_type || "audio/mpeg" });

      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${trackTitle}.mp3`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);

      this.showStatus(`📥 Downloaded: ${trackTitle}`, "success");
    } catch (e) {
      this.showStatus(`❌ Download failed: ${e.message}`, "error");
    }
  }

  async deleteTrack(trackId, trackTitle) {
    if (!confirm(`Are you sure you want to delete "${trackTitle}"? This cannot be undone.`)) return;

    this.showStatus("🗑️ Deleting track...", "info");

    try {
      const response = await fetch(`/music/${encodeURIComponent(trackId)}`, { method: "DELETE" });
      if (!response.ok) throw new Error(`Delete failed: ${response.status}`);

      // If currently playing, stop
      if (this.currentTrackId === trackId) {
        await this.stopTrack(trackId);
      }

      this.musicLibrary = this.musicLibrary.filter((t) => t.key !== trackId);
      this.renderTracks();
      this.showStatus(`🗑️ Deleted: ${trackTitle}`, "success");
    } catch (e) {
      this.showStatus(`❌ Delete failed: ${e.message}`, "error");
    }
  }

  // -----------------------------
  // Upload
  // -----------------------------
  async handleFileUpload(files) {
    const audioFiles = files.filter((f) => f.type?.startsWith("audio/"));
    if (audioFiles.length === 0) {
      this.showStatus("Please select audio files only", "error");
      return;
    }

    this.closeUploadModal();

    for (const file of audioFiles) {
      await this.uploadFile(file);
    }

    await this.loadMusicLibrary();
  }

  async uploadFile(file) {
    this.showStatus(`📤 Uploading ${file.name}...`, "info");

    try {
      const formData = new FormData();
      formData.append("file", file);

      const response = await fetch("/music", {
        method: "POST",
        headers: { "x-service-method": "upload" },
        body: formData,
      });

      if (!response.ok) throw new Error(`Upload failed: ${response.status}`);

      await response.json().catch(() => null);
      this.showStatus(`✅ Uploaded: ${file.name}`, "success");
    } catch (e) {
      this.showStatus(`❌ Upload failed: ${file.name}`, "error");
    }
  }

  openUploadModal() {
    document.getElementById("uploadModal")?.classList.add("active");
  }

  closeUploadModal() {
    document.getElementById("uploadModal")?.classList.remove("active");
  }

  // -----------------------------
  // Search / Sort / View
  // -----------------------------
  filterTracks(searchTerm) {
    const term = String(searchTerm || "").toLowerCase();
    const tracks = document.querySelectorAll(".track-item");

    tracks.forEach((track) => {
      const title = track.querySelector(".track-title")?.textContent?.toLowerCase() || "";
      const artist = track.querySelector(".track-artist")?.textContent?.toLowerCase() || "";
      track.style.display = title.includes(term) || artist.includes(term) ? "grid" : "none";
    });
  }

  sortTracks(sortBy) {
    // Optional: implement later
    // sortBy values in your HTML: Recently Added / Name A-Z / Duration / Size
  }

  toggleView(view) {
    const trackList = document.getElementById("trackList");
    if (!trackList) return;
    if (view === "grid") trackList.classList.add("grid-view");
    else trackList.classList.remove("grid-view");
  }

  toggleFavorite() {
    const btn = document.getElementById("favoriteBtn");
    const icon = btn?.querySelector("i");
    if (!btn || !icon) return;

    if (icon.classList.contains("far")) {
      icon.className = "fas fa-heart";
      btn.classList.add("active");
    } else {
      icon.className = "far fa-heart";
      btn.classList.remove("active");
    }
  }

  downloadCurrentTrack() {
    if (!this.currentTrackId) return;
    const track = this.musicLibrary.find((t) => t.key === this.currentTrackId); // FIX: key not id
    const title = track?.metadata?.title || track?.filename || "Unknown Title";
    this.downloadTrack(this.currentTrackId, title);
  }

  // -----------------------------
  // Utils
  // -----------------------------
  base64ToBytes(b64) {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return bytes;
  }

  formatDuration(seconds) {
    if (!seconds || isNaN(seconds)) return "0:00";
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${String(s).padStart(2, "0")}`;
  }

  showStatus(message, type = "info") {
    document.querySelectorAll(".status-message").forEach((m) => m.remove());
    const el = document.createElement("div");
    el.className = `status-message ${type}`;
    el.textContent = message;
    document.body.appendChild(el);

    setTimeout(() => el.classList.add("show"), 80);
    setTimeout(() => {
      el.classList.remove("show");
      setTimeout(() => el.remove(), 250);
    }, 3000);
  }
}

// Init
document.addEventListener("DOMContentLoaded", () => {
  window.musicPlayer = new MusicPlayer();

  const mobileMenuBtn = document.getElementById("mobileMenuBtn");
  const sidebar = document.querySelector(".sidebar");

  if (mobileMenuBtn && sidebar) {
    mobileMenuBtn.addEventListener("click", () => sidebar.classList.toggle("open"));

    document.addEventListener("click", (e) => {
      if (
        window.innerWidth <= 1024 &&
        sidebar.classList.contains("open") &&
        !sidebar.contains(e.target) &&
        !mobileMenuBtn.contains(e.target)
      ) {
        sidebar.classList.remove("open");
      }
    });

    window.addEventListener("resize", () => {
      if (window.innerWidth > 1024) sidebar.classList.remove("open");
    });
  }
});

// Close modal and dropdowns on outside click
document.addEventListener("click", (e) => {
  const modal = document.getElementById("uploadModal");
  if (e.target === modal) closeUploadModal();


});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeUploadModal();
});