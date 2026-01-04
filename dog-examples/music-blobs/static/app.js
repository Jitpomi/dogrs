// Modern Music Player JavaScript with Real-Time Audio Visualization

// Global functions that need to be available immediately
function closeUploadModal() {
    document.getElementById('uploadModal').classList.remove('active');
}

// Ensure function is available on window object
window.closeUploadModal = closeUploadModal;

class MusicPlayer {
    constructor() {
        this.musicLibrary = [];
        this.currentAudio = null;
        this.currentTrackId = null;
        this.isPlaying = false;
        this.isPaused = false;
        this.currentTime = 0;
        this.duration = 0;
        this.waveformCanvases = new Map();
        this.audioContext = null;
        this.analyser = null;
        this.dataArray = null;
        this.animationId = null;
        
        this.init();
    }

    init() {
        this.setupEventListeners();
        this.loadMusicLibrary();
        this.generateWaveforms();
    }

    setupEventListeners() {
        // Upload button
        document.querySelector('.upload-btn').addEventListener('click', () => {
            this.openUploadModal();
        });

        // Upload area
        const uploadArea = document.getElementById('uploadArea');
        const fileInput = document.getElementById('fileInput');

        uploadArea.addEventListener('click', () => {
            fileInput.click();
        });

        uploadArea.addEventListener('dragover', (e) => {
            e.preventDefault();
            uploadArea.style.borderColor = 'var(--accent-primary)';
            uploadArea.style.background = 'rgba(29, 185, 84, 0.1)';
        });

        uploadArea.addEventListener('dragleave', (e) => {
            e.preventDefault();
            uploadArea.style.borderColor = 'var(--border-color)';
            uploadArea.style.background = 'transparent';
        });

        uploadArea.addEventListener('drop', (e) => {
            e.preventDefault();
            uploadArea.style.borderColor = 'var(--border-color)';
            uploadArea.style.background = 'transparent';
            
            const files = Array.from(e.dataTransfer.files);
            this.handleFileUpload(files);
        });

        fileInput.addEventListener('change', (e) => {
            const files = Array.from(e.target.files);
            this.handleFileUpload(files);
        });

        // Search functionality
        document.querySelector('.search-input').addEventListener('input', (e) => {
            this.filterTracks(e.target.value);
        });

        // Filter selectors
        document.querySelectorAll('.filter-select').forEach(select => {
            select.addEventListener('change', () => {
                this.applyFilters();
            });
        });

        // Sort selector
        document.querySelector('.sort-select').addEventListener('change', (e) => {
            this.sortTracks(e.target.value);
        });

        // View toggle
        document.querySelectorAll('.view-btn').forEach(btn => {
            btn.addEventListener('click', (e) => {
                document.querySelectorAll('.view-btn').forEach(b => b.classList.remove('active'));
                e.target.closest('.view-btn').classList.add('active');
                this.toggleView(e.target.closest('.view-btn').dataset.view);
            });
        });

        // Player controls
        document.getElementById('playPauseBtn').addEventListener('click', () => {
            this.togglePlayPause();
        });

        document.getElementById('prevBtn').addEventListener('click', () => {
            this.previousTrack();
        });

        document.getElementById('nextBtn').addEventListener('click', () => {
            this.nextTrack();
        });

        document.getElementById('downloadBtn').addEventListener('click', () => {
            this.downloadCurrentTrack();
        });

        document.getElementById('favoriteBtn').addEventListener('click', () => {
            this.toggleFavorite();
        });

        // Progress bar
        const progressBar = document.querySelector('.progress-bar');
        progressBar.addEventListener('click', (e) => {
            this.seekTo(e);
        });
    }

    async loadMusicLibrary() {
        try {
            this.showStatus('Loading music library...', 'info');
            
            const response = await fetch('/music', {
                method: 'GET',
                headers: {
                    'Content-Type': 'application/json'
                }
            });

            if (!response.ok) {
                throw new Error(`Failed to load library: ${response.status}`);
            }

            const data = await response.json();
            this.musicLibrary = Array.isArray(data) ? data : [];
            
            this.renderTracks();
            this.showStatus(`Loaded ${this.musicLibrary.length} tracks`, 'success');
            
        } catch (error) {
            this.showStatus('Failed to load music library', 'error');
            this.musicLibrary = [];
            this.renderTracks();
        }
    }

    renderTracks() {
        const trackList = document.getElementById('trackList');
        
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
                </div>
            `;
            return;
        }

        trackList.innerHTML = this.musicLibrary.map(track => {
            const trackId = track.key; // Backend uses 'key' field, not 'id'
            const title = track.metadata?.title || track.filename || 'Unknown Title';
            const artist = track.metadata?.artist || 'Unknown Artist';
            const duration = this.formatDuration(track.metadata?.duration || 0);
            const genre = track.metadata?.genre || 'Unknown';
            const sizeInMB = (track.size_bytes / 1024 / 1024).toFixed(1); // Backend uses 'size_bytes'
            
            return `
                <div class="track-item ${this.currentTrackId === trackId ? 'playing' : ''}" data-track-id="${trackId}">
                    <div class="track-index">
                        <span class="index-number">${this.musicLibrary.indexOf(track) + 1}</span>
                    </div>
                    
                    <div class="track-play-btn">
                        <button class="play-button play-btn" data-track-id="${trackId}" style="display: inline-block;">
                            <i class="fas fa-play"></i>
                        </button>
                        <button class="play-button pause-btn" data-track-id="${trackId}" style="display: none;">
                            <i class="fas fa-pause"></i>
                        </button>
                        <button class="play-button stop-btn" data-track-id="${trackId}" style="display: none;">
                            <i class="fas fa-stop"></i>
                        </button>
                    </div>
                    
                    <div class="track-artwork">
                        ${track.metadata?.album_art_url ? 
                            `<img src="${track.metadata.album_art_url}" alt="Album Art">` :
                            `<div class="placeholder"><i class="fas fa-music"></i></div>`
                        }
                    </div>
                    
                    <div class="track-info">
                        <div class="track-title">${title} <span class="track-badge">NEW</span></div>
                        <div class="track-artist">${artist}</div>
                    </div>
                    
                    <div class="track-dropdown">
                        <button class="dropdown-btn">
                            <i class="fas fa-chevron-down"></i>
                        </button>
                    </div>
                    
                    <div class="track-genre-tag">
                        <span class="genre-badge">${genre}</span>
                    </div>
                    
                    <div class="track-vocal-indicator">
                        <i class="fas fa-microphone"></i>
                    </div>
                    
                    <div class="track-tempo">
                        <i class="fas fa-music"></i>
                    </div>
                    
                    <div class="track-duration">${duration}</div>
                    
                    <div class="track-waveform">
                        ${this.createWaveformContainer(trackId)}
                    </div>
                    
                    <div class="track-actions">
                        <button class="action-btn favorite-btn">
                            <i class="far fa-heart"></i>
                        </button>
                        <button class="action-btn star-btn">
                            <i class="fas fa-star"></i>
                        </button>
                        <div class="track-menu-container">
                            <button class="action-btn menu-btn" data-track-id="${trackId}">
                                <i class="fas fa-ellipsis-v"></i>
                            </button>
                        </div>
                    </div>
                </div>`;
        }).join('');

        // Add click listeners for all button types
        document.querySelectorAll('.play-btn').forEach(button => {
            button.addEventListener('click', (e) => {
                e.stopPropagation();
                const trackId = button.dataset.trackId;
                this.playTrack(trackId);
            });
        });

        document.querySelectorAll('.stop-btn').forEach(button => {
            button.addEventListener('click', (e) => {
                e.stopPropagation();
                const trackId = button.dataset.trackId;
                this.stopTrack(trackId);
            });
        });

        document.querySelectorAll('.pause-btn').forEach(button => {
            button.addEventListener('click', (e) => {
                e.stopPropagation();
                const trackId = button.dataset.trackId;
                this.pauseTrack(trackId);
            });
        });

        // Add menu button event listeners
        document.querySelectorAll('.menu-btn').forEach(button => {
            button.addEventListener('click', (e) => {
                e.stopPropagation();
                const trackId = button.dataset.trackId;
                
                // Close any existing dropdown
                const existingMenu = document.querySelector('.track-dropdown-menu.active');
                if (existingMenu) {
                    existingMenu.remove();
                }
                
                // Create new dropdown menu
                const menu = document.createElement('div');
                menu.className = 'track-dropdown-menu active';
                menu.innerHTML = `
                    <div class="dropdown-item download-item" data-track-id="${trackId}">
                        <i class="fas fa-download"></i>
                        <span>Download</span>
                    </div>
                    <div class="dropdown-item delete-item" data-track-id="${trackId}">
                        <i class="fas fa-trash"></i>
                        <span>Delete</span>
                    </div>
                `;
                
                // Position menu relative to button
                const rect = button.getBoundingClientRect();
                menu.style.position = 'fixed';
                menu.style.top = `${rect.bottom + 5}px`;
                menu.style.left = `${rect.right - 140}px`;
                menu.style.zIndex = '2147483647';
                
                // Append to body to escape stacking context
                document.body.appendChild(menu);
                
                // Add event listeners to menu items
                menu.querySelector('.download-item').addEventListener('click', (e) => {
                    e.stopPropagation();
                    const trackItem = document.querySelector(`[data-track-id="${trackId}"]`);
                    const trackTitle = trackItem.querySelector('.track-title').textContent.replace(' NEW', '').trim();
                    this.downloadTrack(trackId, trackTitle);
                    menu.remove();
                });
                
                menu.querySelector('.delete-item').addEventListener('click', (e) => {
                    e.stopPropagation();
                    const trackItem = document.querySelector(`[data-track-id="${trackId}"]`);
                    const trackTitle = trackItem.querySelector('.track-title').textContent.replace(' NEW', '').trim();
                    this.deleteTrack(trackId, trackTitle);
                    menu.remove();
                });
            });
        });


        // Generate real waveforms immediately after rendering
        setTimeout(() => {
            this.generateWaveforms();
        }, 100);
    }

    createWaveformContainer(trackId) {
        return `<canvas class="track-waveform-canvas" id="waveform-${trackId}" width="640" height="80"></canvas>`;
    }

    initializeRealTimeWaveform(trackId) {
        const canvas = document.getElementById(`waveform-${trackId}`);
        if (!canvas) {
            return null;
        }

        const ctx = canvas.getContext('2d');
        const width = canvas.width;
        const height = canvas.height;

        // Store canvas and context for this track
        this.waveformCanvases.set(trackId, { canvas, ctx, width, height });

        // Add click-to-seek functionality
        canvas.addEventListener('click', (e) => {
            if (this.currentTrackId === trackId && this.currentAudio) {
                const rect = canvas.getBoundingClientRect();
                const x = e.clientX - rect.left;
                const progress = x / rect.width;
                const seekTime = progress * this.currentAudio.duration;
                this.currentAudio.currentTime = seekTime;
            }
        });

        // Add hover effects
        canvas.addEventListener('mouseenter', () => {
            canvas.style.cursor = 'pointer';
            canvas.style.opacity = '1';
        });

        canvas.addEventListener('mouseleave', () => {
            canvas.style.cursor = 'default';
            canvas.style.opacity = '0.9';
        });

        // Initialize with static pattern until audio plays
        this.drawStaticWaveform(ctx, width, height);
        
        return { canvas, ctx, width, height };
    }

    drawStaticWaveform(ctx, width, height) {
        ctx.clearRect(0, 0, width, height);
        
        // Scale for high DPI like the animated version
        const dpr = window.devicePixelRatio || 1;
        ctx.scale(dpr, dpr);
        
        // Transparent background to match Artlist
        ctx.fillStyle = 'rgba(0, 0, 0, 0)';
        ctx.fillRect(0, 0, width, height);
        
        // Generate ultra-detailed Artlist-style static waveform
        const barWidth = 0.5;
        const barGap = 0.1;
        const totalBarWidth = barWidth + barGap;
        const barCount = Math.floor(width / totalBarWidth);
        
        // Create realistic audio waveform pattern with multiple frequency layers
        for (let i = 0; i < barCount; i++) {
            const x = i * totalBarWidth;
            const normalizedPos = i / barCount;
            
            // Layer multiple sine waves to create realistic audio patterns
            const bassFreq = Math.sin(normalizedPos * Math.PI * 4) * 0.3;
            const midFreq = Math.sin(normalizedPos * Math.PI * 12) * 0.2;
            const highFreq = Math.sin(normalizedPos * Math.PI * 32) * 0.15;
            const noise = (Math.random() - 0.5) * 0.1;
            
            // Combine frequencies for realistic waveform
            let amplitude = Math.abs(bassFreq + midFreq + highFreq + noise);
            
            // Add musical structure (verses, chorus patterns)
            const structurePattern = Math.sin(normalizedPos * Math.PI * 2) * 0.2 + 0.8;
            amplitude *= structurePattern;
            
            // Ensure minimum and maximum bounds
            amplitude = Math.max(0.02, Math.min(0.9, amplitude));
            
            const maxBarHeight = height * 0.95;
            const barHeight = amplitude * maxBarHeight;
            const y = (height - barHeight) / 2;
            
            // Artlist-style detailed grayscale with proper density
            const intensity = amplitude;
            const baseAlpha = 0.6 + (intensity * 0.4);
            
            // Create highly detailed gradient matching Artlist's quality
            const gradient = ctx.createLinearGradient(x, y, x, y + barHeight);
            gradient.addColorStop(0, `rgba(200, 210, 220, ${baseAlpha})`);
            gradient.addColorStop(0.3, `rgba(180, 190, 200, ${baseAlpha * 0.95})`);
            gradient.addColorStop(0.7, `rgba(160, 170, 180, ${baseAlpha * 0.9})`);
            gradient.addColorStop(1, `rgba(140, 150, 160, ${baseAlpha * 0.85})`);
            
            ctx.fillStyle = gradient;
            ctx.fillRect(x, y, barWidth, barHeight);
            
            // Add micro-details for high-frequency content
            if (amplitude > 0.4) {
                const detailHeight = barHeight * 0.1;
                ctx.fillStyle = `rgba(220, 230, 240, ${intensity * 0.6})`;
                ctx.fillRect(x, y, barWidth, detailHeight);
                ctx.fillRect(x, y + barHeight - detailHeight, barWidth, detailHeight);
            }
            
            // Add subtle variations for realism
            if (i % 3 === 0 && amplitude > 0.2) {
                ctx.fillStyle = `rgba(190, 200, 210, ${intensity * 0.3})`;
                ctx.fillRect(x, y + barHeight * 0.3, barWidth, barHeight * 0.4);
            }
        }
    }

    setupAudioAnalyzer(audioElement) {
        try {
            // Create audio context if it doesn't exist
            if (!this.audioContext) {
                this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
            }

            // Create analyzer node with high-resolution settings like Artlist
            this.analyser = this.audioContext.createAnalyser();
            this.analyser.fftSize = 2048; // Much higher resolution for detailed waveforms
            this.analyser.smoothingTimeConstant = 0.3; // Less smoothing for more responsive visualization
            
            const bufferLength = this.analyser.frequencyBinCount;
            this.dataArray = new Uint8Array(bufferLength);

            // Connect audio element to analyzer
            const source = this.audioContext.createMediaElementSource(audioElement);
            source.connect(this.analyser);
            this.analyser.connect(this.audioContext.destination);

            return true;
        } catch (error) {
            return false;
        }
    }

    startWaveformAnimation(trackId) {
        if (this.animationId) {
            cancelAnimationFrame(this.animationId);
        }

        const waveformData = this.waveformCanvases.get(trackId);
        if (!waveformData || !this.analyser) return;

        const { ctx, width, height } = waveformData;
        
        // Scale canvas for high DPI displays
        const dpr = window.devicePixelRatio || 1;
        ctx.scale(dpr, dpr);

        const animate = () => {
            if (this.currentTrackId !== trackId || !this.isPlaying) {
                return;
            }
            
            // Get high-resolution frequency data
            this.analyser.getByteFrequencyData(this.dataArray);

            // Clear canvas with dark background like Artlist
            ctx.fillStyle = 'rgba(20, 20, 20, 0.8)';
            ctx.fillRect(0, 0, width, height);

            // Draw Artlist-style detailed waveform
            const barWidth = 0.8;
            const barGap = 0.2;
            const totalBarWidth = barWidth + barGap;
            const barCount = Math.min(Math.floor(width / totalBarWidth), this.dataArray.length);
            
            // Create detailed frequency visualization like Artlist
            for (let i = 0; i < barCount; i++) {
                const x = i * totalBarWidth;
                
                // Use multiple frequency bins for more detail
                const binIndex = Math.floor((i / barCount) * this.dataArray.length);
                const frequency = this.dataArray[binIndex] / 255;
                
                // Calculate bar height with more precision
                const maxBarHeight = height * 0.85;
                const barHeight = Math.max(0.5, frequency * maxBarHeight);
                const y = (height - barHeight) / 2;
                
                // Artlist-style grayscale with subtle blue tint
                const intensity = frequency;
                const alpha = 0.3 + (intensity * 0.7);
                
                // Create subtle gradient for each bar
                const gradient = ctx.createLinearGradient(x, y, x, y + barHeight);
                gradient.addColorStop(0, `rgba(180, 190, 200, ${alpha})`);
                gradient.addColorStop(0.5, `rgba(160, 170, 180, ${alpha * 0.9})`);
                gradient.addColorStop(1, `rgba(140, 150, 160, ${alpha * 0.8})`);
                
                ctx.fillStyle = gradient;
                ctx.fillRect(x, y, barWidth, barHeight);
                
                // Add subtle highlight for high frequencies
                if (frequency > 0.6) {
                    ctx.fillStyle = `rgba(200, 210, 220, ${frequency * 0.4})`;
                    ctx.fillRect(x, y, barWidth, Math.max(1, barHeight * 0.3));
                }
            }

            // Draw professional progress indicator like Artlist
            if (this.currentAudio && this.currentAudio.duration > 0) {
                const progress = this.currentAudio.currentTime / this.currentAudio.duration;
                const progressX = progress * width;
                
                // Progress overlay with subtle teal tint
                ctx.fillStyle = 'rgba(0, 212, 170, 0.15)';
                ctx.fillRect(0, 0, progressX, height);
                
                // Clean progress line like Artlist
                ctx.fillStyle = '#00d4aa';
                ctx.fillRect(progressX - 0.5, 0, 1, height);
            }

            this.animationId = requestAnimationFrame(animate);
        };

        animate();
    }

    stopWaveformAnimation() {
        if (this.animationId) {
            cancelAnimationFrame(this.animationId);
            this.animationId = null;
        }
    }

    enhanceWaveformAppearance(container) {
        // Apply CSS filters and styling to make waveform look more like Artlist
        const waveCanvas = container.querySelector('canvas');
        if (waveCanvas) {
            waveCanvas.style.filter = 'contrast(1.3) brightness(1.2)';
            waveCanvas.style.opacity = '1';
        }
        
        // Add Artlist-style background and progress overlay
        container.style.background = 'rgba(255, 255, 255, 0.03)';
        container.style.borderRadius = '2px';
        container.style.position = 'relative';
        container.style.overflow = 'hidden';
        
        // Create progress overlay element
        const progressOverlay = document.createElement('div');
        progressOverlay.className = 'waveform-progress-overlay';
        progressOverlay.style.cssText = `
            position: absolute;
            top: 0;
            left: 0;
            height: 100%;
            width: 0%;
            background: linear-gradient(90deg, rgba(0, 212, 170, 0.3) 0%, rgba(0, 212, 170, 0.1) 100%);
            pointer-events: none;
            transition: width 0.1s ease-out;
            z-index: 1;
        `;
        container.appendChild(progressOverlay);

        // Create hover seek indicator
        const hoverIndicator = document.createElement('div');
        hoverIndicator.className = 'waveform-hover-indicator';
        hoverIndicator.style.cssText = `
            position: absolute;
            top: 0;
            left: 0;
            height: 100%;
            width: 2px;
            background: rgba(0, 212, 170, 0.8);
            pointer-events: none;
            opacity: 0;
            transition: opacity 0.2s ease;
            z-index: 2;
        `;
        container.appendChild(hoverIndicator);

        // Add mouse tracking for hover indicator
        container.addEventListener('mousemove', (e) => {
            const rect = container.getBoundingClientRect();
            const x = e.clientX - rect.left;
            const percentage = (x / rect.width) * 100;
            hoverIndicator.style.left = `${Math.max(0, Math.min(100, percentage))}%`;
            hoverIndicator.style.opacity = '1';
        });

        container.addEventListener('mouseleave', () => {
            hoverIndicator.style.opacity = '0';
        });
    }

    createFallbackWaveform(container) {
        // Create a realistic-looking fallback waveform when audio loading fails
        container.innerHTML = '';
        const canvas = document.createElement('canvas');
        canvas.width = 280;
        canvas.height = 32;
        canvas.style.width = '100%';
        canvas.style.height = '32px';
        
        const ctx = canvas.getContext('2d');
        ctx.fillStyle = 'rgba(255, 255, 255, 0.4)';
        
        // Generate realistic waveform pattern
        const barWidth = 1;
        const barCount = Math.floor(canvas.width / (barWidth + 1));
        
        for (let i = 0; i < barCount; i++) {
            const x = i * (barWidth + 1);
            const amplitude = Math.random() * 0.8 + 0.2; // Random but realistic amplitude
            const height = Math.floor(canvas.height * amplitude);
            const y = (canvas.height - height) / 2;
            
            ctx.fillRect(x, y, barWidth, height);
        }
        
        container.appendChild(canvas);
        this.enhanceWaveformAppearance(container);
    }

    async playTrack(trackId) {
        
        try {
            // If same track and paused, resume
            if (this.currentTrackId === trackId && this.isPaused && this.currentAudio) {
                this.currentAudio.play();
                this.isPlaying = true;
                this.isPaused = false;
                this.updatePlayButton(trackId, true);
                this.showStatus('▶️ Resumed playback', 'success');
                return;
            }
            
            // Stop current track if playing different one
            if (this.currentAudio && this.currentTrackId !== trackId) {
                this.currentAudio.pause();
                this.currentAudio = null;
                this.updatePlayButton(this.currentTrackId, false);
            }
            
            // Get streaming info from backend
            const streamResponse = await fetch('/music', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'stream'
                },
                body: JSON.stringify({ key: trackId })
            });
            
            if (!streamResponse.ok) {
                throw new Error(`Failed to start stream: ${streamResponse.status}`);
            }
            
            const streamInfo = await streamResponse.json();
            
            // Create audio element and start playback
            this.currentAudio = new Audio();
            this.currentTrackId = trackId;
            
            // Set up audio event listeners
            this.setupAudioEventListeners(trackId);
            
            // Handle the stream response
            const track = this.musicLibrary.find(t => t.key === trackId);
            if (track && streamInfo.audio_data) {
                
                // Convert base64 to blob
                const binaryString = atob(streamInfo.audio_data);
                const bytes = new Uint8Array(binaryString.length);
                for (let i = 0; i < binaryString.length; i++) {
                    bytes[i] = binaryString.charCodeAt(i);
                }
                const blob = new Blob([bytes], { type: streamInfo.content_type || 'audio/mpeg' });
                const audioUrl = URL.createObjectURL(blob);
                
                this.currentAudio.src = audioUrl;
                
                // Initialize real-time waveform for this track if not already done
                if (!this.waveformCanvases.has(trackId)) {
                    this.initializeRealTimeWaveform(trackId);
                }
                
                // Setup audio analyzer for real-time visualization
                this.setupAudioAnalyzer(this.currentAudio);
                
                this.currentAudio.play();
                
                this.showAudioPlayer(track);
                
            } else {
                throw new Error('No audio data available in stream response');
            }
            
        } catch (error) {
            this.showStatus(`❌ Failed to play track: ${error.message}`, 'error');
            this.updatePlayButton(trackId, false);
        }
    }

    setupAudioEventListeners(trackId) {
        this.currentAudio.addEventListener('loadstart', () => {
            this.showStatus('🎵 Loading track...', 'info');
        });
        
        this.currentAudio.addEventListener('canplay', () => {
            this.showStatus('🎵 Playing track', 'success');
            this.updatePlayButton(trackId, true);
        });
        
        this.currentAudio.addEventListener('play', () => {
            this.isPlaying = true;
            this.updatePlayButton(trackId, true);
            this.updatePlayerControls();
            // Start real-time waveform animation
            this.startWaveformAnimation(trackId);
        });
        
        this.currentAudio.addEventListener('pause', () => {
            this.isPlaying = false;
            this.updatePlayButton(trackId, false);
            this.updatePlayerControls();
            // Stop real-time waveform animation
            this.stopWaveformAnimation();
        });
        
        this.currentAudio.addEventListener('ended', () => {
            this.isPlaying = false;
            this.currentAudio = null;
            this.currentTrackId = null;
            this.updatePlayButton(trackId, false);
            this.hideAudioPlayer();
            this.showStatus('🎵 Track finished', 'info');
        });
        
        this.currentAudio.addEventListener('error', (e) => {
            this.updatePlayButton(trackId, false);
        });

        this.currentAudio.addEventListener('timeupdate', () => {
            this.updateProgress();
            this.syncWaveformWithAudio(trackId);
        });

        this.currentAudio.addEventListener('loadedmetadata', () => {
            this.duration = this.currentAudio.duration;
            this.updateProgress();
        });
    }

    syncWaveformWithAudio(trackId) {
        // Real-time waveform is handled by the animation loop
        // This function is kept for compatibility but not needed
        return;
    }

    updatePlayButton(trackId, playing) {
        const trackItem = document.querySelector(`[data-track-id="${trackId}"]`);
        if (!trackItem) return;

        const playBtn = trackItem.querySelector('.play-btn');
        const pauseBtn = trackItem.querySelector('.pause-btn');
        const stopBtn = trackItem.querySelector('.stop-btn');
        const deleteBtn = trackItem.querySelector('.delete-btn');

        if (playing) {
            // Playing state: show stop button only, hide play + pause + delete
            if (playBtn) playBtn.style.display = 'none';
            if (pauseBtn) pauseBtn.style.display = 'none';
            if (stopBtn) stopBtn.style.display = 'inline-block';
            if (deleteBtn) deleteBtn.style.display = 'none';
        } else {
            // Not playing state: show play + delete buttons, hide pause + stop
            if (playBtn) playBtn.style.display = 'inline-block';
            if (pauseBtn) pauseBtn.style.display = 'none';
            if (stopBtn) stopBtn.style.display = 'none';
            if (deleteBtn) deleteBtn.style.display = 'inline-block';
        }

        // Reset all track visual states
        document.querySelectorAll('.track-item').forEach(item => {
            item.classList.remove('playing');
        });

        if (playing) {
            trackItem.classList.add('playing');
        }
    }

    showAudioPlayer(track) {
        const player = document.getElementById('audioPlayer');
        const artwork = document.getElementById('playerArtwork');
        const title = document.getElementById('playerTitle');
        const artist = document.getElementById('playerArtist');

        artwork.src = track.metadata?.album_art_url || '';
        artwork.style.display = track.metadata?.album_art_url ? 'block' : 'none';
        title.textContent = track.metadata?.title || track.filename || 'Unknown Title';
        artist.textContent = track.metadata?.artist || 'Unknown Artist';

        player.style.display = 'grid';
        this.updatePlayerControls();
    }

    hideAudioPlayer() {
        document.getElementById('audioPlayer').style.display = 'none';
    }

    updatePlayerControls() {
        const playPauseBtn = document.getElementById('playPauseBtn');
        const icon = playPauseBtn.querySelector('i');
        
        if (this.isPlaying) {
            icon.className = 'fas fa-pause';
        } else {
            icon.className = 'fas fa-play';
        }
    }

    updateProgress() {
        if (!this.currentAudio) return;

        const currentTime = this.currentAudio.currentTime;
        const duration = this.currentAudio.duration || 0;
        const progress = duration > 0 ? (currentTime / duration) * 100 : 0;

        document.getElementById('progressFill').style.width = `${progress}%`;
        document.getElementById('progressHandle').style.left = `${progress}%`;
        document.getElementById('currentTime').textContent = this.formatDuration(currentTime);
        document.getElementById('totalTime').textContent = this.formatDuration(duration);
    }

    async togglePlayPause() {
        if (!this.currentAudio || !this.currentTrackId) return;

        try {
            if (this.isPlaying) {
                // Pause the track
                await this.pauseTrack(this.currentTrackId);
            } else if (this.isPaused) {
                // Resume the track
                await this.resumeTrack(this.currentTrackId);
            } else {
                // Start playing if not already playing
                this.currentAudio.play();
            }
        } catch (error) {
            this.showStatus(`❌ Playback error: ${error.message}`, 'error');
        }
    }

    async pauseTrack(trackId) {
        
        try {
            const response = await fetch('/music', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'pause'
                },
                body: JSON.stringify({ key: trackId })
            });

            if (!response.ok) {
                throw new Error(`Failed to pause: ${response.status}`);
            }

            // Pause frontend audio
            if (this.currentAudio) {
                this.currentAudio.pause();
                this.isPlaying = false;
                this.isPaused = true;
                this.updatePlayButton(trackId, false);
                this.updatePlayerControls();
                this.showStatus('⏸️ Paused', 'info');
            }

        } catch (error) {
            this.showStatus(`❌ Pause failed: ${error.message}`, 'error');
        }
    }

    async resumeTrack(trackId) {
        
        try {
            const response = await fetch('/music', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'resume'
                },
                body: JSON.stringify({ key: trackId })
            });

            if (!response.ok) {
                throw new Error(`Failed to resume: ${response.status}`);
            }

            // Resume frontend audio
            if (this.currentAudio) {
                this.currentAudio.play();
                this.isPlaying = true;
                this.isPaused = false;
                this.updatePlayButton(trackId, true);
                this.updatePlayerControls();
                this.showStatus('▶️ Resumed', 'success');
            }

        } catch (error) {
            this.showStatus(`❌ Resume failed: ${error.message}`, 'error');
        }
    }

    async stopTrack(trackId) {
        
        try {
            const response = await fetch('/music', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'cancel'
                },
                body: JSON.stringify({ key: trackId })
            });

            if (!response.ok) {
                throw new Error(`Failed to stop: ${response.status}`);
            }

            // Stop frontend audio and reset state
            if (this.currentAudio) {
                this.currentAudio.pause();
                this.currentAudio.currentTime = 0;
                this.currentAudio.src = '';
                this.currentAudio.load(); // Reset the audio element
                this.currentAudio = null;
            }
            
            // Stop waveform animation
            this.stopWaveformAnimation();
            
            this.currentTrackId = null;
            this.isPlaying = false;
            this.isPaused = false;
            this.updatePlayButton(trackId, false);
            this.updatePlayerControls();
            this.hideAudioPlayer();
            this.showStatus('⏹️ Stopped', 'info');

        } catch (error) {
            this.showStatus(`❌ Stop failed: ${error.message}`, 'error');
        }
    }

    seekTo(e) {
        if (!this.currentAudio || !this.duration) return;

        const progressBar = e.currentTarget;
        const rect = progressBar.getBoundingClientRect();
        const clickX = e.clientX - rect.left;
        const percentage = clickX / rect.width;
        const newTime = percentage * this.duration;

        this.currentAudio.currentTime = newTime;
    }

    previousTrack() {
        if (!this.currentTrackId) return;

        const currentIndex = this.musicLibrary.findIndex(track => track.key === this.currentTrackId);
        if (currentIndex > 0) {
            this.playTrack(this.musicLibrary[currentIndex - 1].key);
        }
    }

    nextTrack() {
        if (!this.currentTrackId) return;

        const currentIndex = this.musicLibrary.findIndex(track => track.key === this.currentTrackId);
        if (currentIndex < this.musicLibrary.length - 1) {
            this.playTrack(this.musicLibrary[currentIndex + 1].key);
        }
    }

    async downloadTrack(trackId, trackTitle) {
        this.showStatus('📥 Starting download...', 'info');

        try {
            const response = await fetch('/music', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'stream'
                },
                body: JSON.stringify({ key: trackId })
            });

            if (!response.ok) {
                throw new Error(`Download failed: ${response.status}`);
            }

            // Backend stream method returns JSON with audio_data as base64
            const streamInfo = await response.json();
            
            if (streamInfo.audio_data) {
                // Convert base64 to blob and download
                const binaryString = atob(streamInfo.audio_data);
                const bytes = new Uint8Array(binaryString.length);
                for (let i = 0; i < binaryString.length; i++) {
                    bytes[i] = binaryString.charCodeAt(i);
                }
                const blob = new Blob([bytes], { type: streamInfo.content_type || 'audio/mpeg' });
                
                // Create download link
                const url = URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url;
                a.download = `${trackTitle}.mp3`;
                document.body.appendChild(a);
                a.click();
                document.body.removeChild(a);
                URL.revokeObjectURL(url);
                
                this.showStatus(`📥 Downloaded: ${trackTitle}`, 'success');
            } else {
                throw new Error('No audio data available for download');
            }

        } catch (error) {
            this.showStatus(`❌ Download failed: ${error.message}`, 'error');
        }
    }

    async deleteTrack(trackId, trackTitle) {
        if (!confirm(`Are you sure you want to delete "${trackTitle}"? This cannot be undone.`)) {
            return;
        }

        this.showStatus('🗑️ Deleting track...', 'info');

        try {
            const response = await fetch(`/music/${encodeURIComponent(trackId)}`, {
                method: 'DELETE'
            });

            if (!response.ok) {
                throw new Error(`Delete failed: ${response.status}`);
            }

            // Remove from library and re-render
            this.musicLibrary = this.musicLibrary.filter(track => track.key !== trackId);
            this.renderTracks();
            
            // Stop if currently playing
            if (this.currentTrackId === trackId) {
                if (this.currentAudio) {
                    this.currentAudio.pause();
                    this.currentAudio = null;
                }
                this.currentTrackId = null;
                this.isPlaying = false;
                this.hideAudioPlayer();
            }

            this.showStatus(`🗑️ Deleted: ${trackTitle}`, 'success');

        } catch (error) {
            this.showStatus(`❌ Delete failed: ${error.message}`, 'error');
        }
    }

    async handleFileUpload(files) {
        const audioFiles = files.filter(file => file.type.startsWith('audio/'));
        
        if (audioFiles.length === 0) {
            this.showStatus('Please select audio files only', 'error');
            return;
        }

        this.closeUploadModal();
        
        for (const file of audioFiles) {
            await this.uploadFile(file);
        }
        
        // Reload library after uploads
        await this.loadMusicLibrary();
    }

    async uploadFile(file) {
        this.showStatus(`📤 Uploading ${file.name}...`, 'info');

        try {
            const formData = new FormData();
            formData.append('file', file);

            const response = await fetch('/music', {
                method: 'POST',
                headers: {
                    'x-service-method': 'upload'
                },
                body: formData
            });

            if (!response.ok) {
                throw new Error(`Upload failed: ${response.status}`);
            }

            const result = await response.json();
            this.showStatus(`✅ Uploaded: ${file.name}`, 'success');

        } catch (error) {
            this.showStatus(`❌ Upload failed: ${file.name}`, 'error');
        }
    }

    openUploadModal() {
        document.getElementById('uploadModal').classList.add('active');
    }

    closeUploadModal() {
        document.getElementById('uploadModal').classList.remove('active');
    }

    filterTracks(searchTerm) {
        const tracks = document.querySelectorAll('.track-item');
        const term = searchTerm.toLowerCase();

        tracks.forEach(track => {
            const title = track.querySelector('.track-title').textContent.toLowerCase();
            const meta = track.querySelector('.track-meta').textContent.toLowerCase();
            
            if (title.includes(term) || meta.includes(term)) {
                track.style.display = 'grid';
            } else {
                track.style.display = 'none';
            }
        });
    }

    applyFilters() {
        // Implementation for genre, mood, duration filters
    }

    sortTracks(sortBy) {
        // Implementation for sorting
    }

    toggleView(view) {
        const trackList = document.getElementById('trackList');
        if (view === 'grid') {
            trackList.classList.add('grid-view');
        } else {
            trackList.classList.remove('grid-view');
        }
    }

    toggleFavorite() {
        const favoriteBtn = document.getElementById('favoriteBtn');
        const icon = favoriteBtn.querySelector('i');
        
        if (icon.classList.contains('far')) {
            icon.className = 'fas fa-heart';
            favoriteBtn.classList.add('active');
        } else {
            icon.className = 'far fa-heart';
            favoriteBtn.classList.remove('active');
        }
    }

    downloadCurrentTrack() {
        if (this.currentTrackId) {
            const track = this.musicLibrary.find(t => t.id === this.currentTrackId);
            if (track) {
                const title = track.metadata?.title || track.filename || 'Unknown Title';
                this.downloadTrack(this.currentTrackId, title);
            }
        }
    }

    formatDuration(seconds) {
        if (!seconds || isNaN(seconds)) return '0:00';
        
        const minutes = Math.floor(seconds / 60);
        const remainingSeconds = Math.floor(seconds % 60);
        return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
    }

    showStatus(message, type = 'info') {
        // Remove existing status messages
        document.querySelectorAll('.status-message').forEach(msg => msg.remove());
        
        const statusDiv = document.createElement('div');
        statusDiv.className = `status-message ${type}`;
        statusDiv.textContent = message;
        document.body.appendChild(statusDiv);
        
        // Show the message
        setTimeout(() => statusDiv.classList.add('show'), 100);
        
        // Hide after 3 seconds
        setTimeout(() => {
            statusDiv.classList.remove('show');
            setTimeout(() => statusDiv.remove(), 300);
        }, 3000);
    }

    async generateWaveforms() {
        // Generate real waveforms from actual audio data like Artlist
        for (const track of this.musicLibrary) {
            const trackId = track.key;
            if (!this.waveformCanvases.has(trackId)) {
                try {
                    // Get actual audio data to generate real waveform
                    await this.generateRealWaveformFromAudio(trackId);
                } catch (error) {
                    // Fallback to canvas initialization
                    setTimeout(() => {
                        this.initializeRealTimeWaveform(trackId);
                    }, 100);
                }
            }
        }
    }

    async generateRealWaveformFromAudio(trackId) {
        try {
            console.log(`🎵 Generating real waveform for ${trackId}`);
            
            // Get audio data from backend
            const response = await fetch('/music', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'stream'
                },
                body: JSON.stringify({ key: trackId })
            });

            if (!response.ok) {
                console.warn(`Failed to fetch audio for ${trackId}`);
                return;
            }

            const streamInfo = await response.json();
            if (!streamInfo.audio_data) {
                console.warn(`No audio data for ${trackId}`);
                return;
            }

            console.log(`🎵 Processing audio data for ${trackId}`);

            // Convert base64 to audio buffer
            const binaryString = atob(streamInfo.audio_data);
            const bytes = new Uint8Array(binaryString.length);
            for (let i = 0; i < binaryString.length; i++) {
                bytes[i] = binaryString.charCodeAt(i);
            }

            // Create audio context for analysis
            const audioContext = new (window.AudioContext || window.webkitAudioContext)();
            const audioBuffer = await audioContext.decodeAudioData(bytes.buffer);

            console.log(`🎵 Audio decoded successfully for ${trackId}, duration: ${audioBuffer.duration}s`);

            // Initialize canvas if not already done
            if (!this.waveformCanvases.has(trackId)) {
                this.initializeRealTimeWaveform(trackId);
            }
            
            const waveformData = this.waveformCanvases.get(trackId);
            if (!waveformData) {
                console.warn(`No canvas data for ${trackId}`);
                return;
            }

            // Generate real waveform from audio buffer
            this.drawRealWaveformFromBuffer(waveformData, audioBuffer);
            console.log(`🎵 Real waveform generated for ${trackId}`);

        } catch (error) {
            console.error(`Failed to generate real waveform for ${trackId}:`, error);
            // Fallback to basic waveform
            if (!this.waveformCanvases.has(trackId)) {
                this.initializeRealTimeWaveform(trackId);
            }
        }
    }

    drawRealWaveformFromBuffer(waveformData, audioBuffer) {
        const { ctx, width, height } = waveformData;
        const channelData = audioBuffer.getChannelData(0); // Get first channel
        const samples = channelData.length;

        // Clear and reset canvas
        ctx.setTransform(1, 0, 0, 1, 0, 0); // Reset any transforms
        ctx.clearRect(0, 0, width, height);

        // Ultra-high density like Artlist
        const barWidth = 0.4;
        const barGap = 0.1;
        const totalBarWidth = barWidth + barGap;
        const barCount = Math.floor(width / totalBarWidth);
        const samplesPerBar = Math.floor(samples / barCount);

        console.log(`Drawing ${barCount} bars from ${samples} samples`);

        for (let i = 0; i < barCount; i++) {
            const x = i * totalBarWidth;
            
            // Get audio segment for this bar
            const startSample = i * samplesPerBar;
            const endSample = Math.min(startSample + samplesPerBar, samples);
            
            // Calculate both RMS and peak for more detailed visualization
            let rmsSum = 0;
            let peak = 0;
            const segmentLength = endSample - startSample;
            
            for (let j = startSample; j < endSample; j++) {
                const sample = Math.abs(channelData[j]);
                rmsSum += sample * sample;
                peak = Math.max(peak, sample);
            }
            
            const rms = Math.sqrt(rmsSum / segmentLength);
            
            // Combine RMS and peak for more realistic waveform
            const combinedAmplitude = (rms * 0.7 + peak * 0.3);
            const amplitude = Math.min(0.95, combinedAmplitude * 12); // Higher scaling for visibility
            
            const barHeight = Math.max(1, amplitude * height * 0.9);
            const y = (height - barHeight) / 2;

            // Artlist-style detailed grayscale
            const intensity = amplitude;
            const baseAlpha = 0.7 + (intensity * 0.3);

            // High-quality gradient matching Artlist
            const gradient = ctx.createLinearGradient(x, y, x, y + barHeight);
            gradient.addColorStop(0, `rgba(220, 230, 240, ${baseAlpha})`);
            gradient.addColorStop(0.3, `rgba(180, 190, 200, ${baseAlpha * 0.95})`);
            gradient.addColorStop(0.7, `rgba(160, 170, 180, ${baseAlpha * 0.9})`);
            gradient.addColorStop(1, `rgba(140, 150, 160, ${baseAlpha * 0.85})`);

            ctx.fillStyle = gradient;
            ctx.fillRect(x, y, barWidth, barHeight);

            // Add micro-details for high-energy sections
            if (amplitude > 0.4) {
                const detailHeight = Math.max(1, barHeight * 0.15);
                ctx.fillStyle = `rgba(240, 250, 255, ${intensity * 0.6})`;
                ctx.fillRect(x, y, barWidth, detailHeight);
                ctx.fillRect(x, y + barHeight - detailHeight, barWidth, detailHeight);
            }

            // Add subtle variations every few bars
            if (i % 4 === 0 && amplitude > 0.2) {
                ctx.fillStyle = `rgba(200, 210, 220, ${intensity * 0.4})`;
                ctx.fillRect(x, y + barHeight * 0.2, barWidth, barHeight * 0.6);
            }
        }

    }
}


// Initialize the music player when DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    window.musicPlayer = new MusicPlayer();
    
    // Mobile sidebar toggle functionality
    const mobileMenuBtn = document.getElementById('mobileMenuBtn');
    const sidebar = document.querySelector('.sidebar');
    
    if (mobileMenuBtn && sidebar) {
        mobileMenuBtn.addEventListener('click', () => {
            sidebar.classList.toggle('open');
        });
        
        // Close sidebar when clicking outside on mobile
        document.addEventListener('click', (e) => {
            if (window.innerWidth <= 1024 && 
                sidebar.classList.contains('open') && 
                !sidebar.contains(e.target) && 
                !mobileMenuBtn.contains(e.target)) {
                sidebar.classList.remove('open');
            }
        });
        
        // Close sidebar on window resize to desktop
        window.addEventListener('resize', () => {
            if (window.innerWidth > 1024) {
                sidebar.classList.remove('open');
            }
        });
    }
});

// Handle clicks outside modal to close
document.addEventListener('click', (e) => {
    const modal = document.getElementById('uploadModal');
    if (e.target === modal) {
        closeUploadModal();
    }
    
    // Close dropdown menus when clicking outside
    if (!e.target.closest('.track-menu-container') && !e.target.closest('.track-dropdown-menu')) {
        const activeMenu = document.querySelector('.track-dropdown-menu.active');
        if (activeMenu) {
            activeMenu.remove();
        }
    }
});

// Handle escape key to close modal
document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
        closeUploadModal();
    }
});
