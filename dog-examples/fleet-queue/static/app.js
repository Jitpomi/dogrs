class FleetCommandPro {
    constructor() {
        this.apiBaseUrl = 'http://127.0.0.1:3036';
        this.tomtomApiKey = 'c1xp5uxxF9W7z0tPjNcQC48nQlABojKH';
        this.map = null;
        this.vehicleMarkers = new Map();
        this.currentView = 'dispatch';
        
        // Enterprise fleet data
        this.data = {
            vehicles: [],
            drivers: [],
            deliveries: [],
            operations: [],
            jobs: {},
            rules: []
        };
        
        // Real-time update intervals
        this.updateIntervals = new Map();
        
        this.init();
    }
    
    async init() {
        
        this.setupEventListeners();
        this.initializeMap();
        await this.loadAllData();
        this.startRealTimeUpdates();
        this.updateUI();
        
    }
    
    setupEventListeners() {
        // Navigation
        document.querySelectorAll('.nav-button').forEach(item => {
            item.addEventListener('click', (e) => {
                const view = e.currentTarget.dataset.view;
                if (view) this.switchView(view);
            });
        });
        
        // Map zoom controls
        this.setupMapControls();
        
        // Drag and drop for dispatch
        this.setupDragAndDrop();
    }
    
    setupMapControls() {
        // Zoom in button
        const zoomInBtn = document.getElementById('zoomIn');
        if (zoomInBtn) {
            zoomInBtn.addEventListener('click', () => {
                if (this.map) {
                    const currentZoom = this.map.getZoom();
                    this.map.setZoom(currentZoom + 1);
                    this.showNotification('Zoomed in', 'info');
                }
            });
        }
        
        // Zoom out button
        const zoomOutBtn = document.getElementById('zoomOut');
        if (zoomOutBtn) {
            zoomOutBtn.addEventListener('click', () => {
                if (this.map) {
                    const currentZoom = this.map.getZoom();
                    this.map.setZoom(currentZoom - 1);
                    this.showNotification('Zoomed out', 'info');
                }
            });
        }
        
        // Center map button
        const centerMapBtn = document.getElementById('centerMap');
        if (centerMapBtn) {
            centerMapBtn.addEventListener('click', () => {
                if (this.map) {
                    this.centerMapOnVehicles();
                    this.showNotification('Centered on fleet', 'info');
                }
            });
        }
    }
    
    setupDragAndDrop() {
        // Make delivery items draggable
        document.addEventListener('dragstart', (e) => {
            if (e.target.classList.contains('delivery-item')) {
                e.target.classList.add('dragging');
                e.dataTransfer.setData('text/plain', e.target.dataset.deliveryId);
            }
        });
        
        document.addEventListener('dragend', (e) => {
            if (e.target.classList.contains('delivery-item')) {
                e.target.classList.remove('dragging');
            }
        });
        
        // Make driver cards drop zones
        document.addEventListener('dragover', (e) => {
            if (e.target.closest('.driver-card')) {
                e.preventDefault();
                e.target.closest('.driver-card').classList.add('drag-over');
            }
        });
        
        document.addEventListener('dragleave', (e) => {
            if (e.target.closest('.driver-card')) {
                e.target.closest('.driver-card').classList.remove('drag-over');
            }
        });
        
        document.addEventListener('drop', (e) => {
            e.preventDefault();
            const driverCard = e.target.closest('.driver-card');
            if (driverCard) {
                driverCard.classList.remove('drag-over');
                const deliveryId = e.dataTransfer.getData('text/plain');
                const driverId = driverCard.dataset.driverId;
                this.assignDeliveryToDriver(deliveryId, driverId);
            }
        });
    }
    
    switchView(viewName) {
        // Only dispatch view is functional now
        if (viewName !== 'dispatch') {
            return;
        }
        
        // Update navigation
        document.querySelectorAll('.nav-button').forEach(item => {
            item.classList.remove('active');
            item.classList.remove('bg-brand-500', 'text-white');
            item.classList.add('text-slate-700', 'hover:bg-slate-100');
        });
        
        const activeNav = document.querySelector(`[data-view="${viewName}"]`);
        if (activeNav) {
            activeNav.classList.add('active');
            activeNav.classList.remove('text-slate-700', 'hover:bg-slate-100');
            activeNav.classList.add('bg-brand-500', 'text-white');
        }
        
        this.currentView = viewName;
        this.updateCurrentView();
    }
    
    async loadAllData() {
        try {
            await Promise.all([
                this.loadVehicles(),
                this.loadDrivers(),
                this.loadDeliveries(),
                this.loadOperations(),
                this.loadJobs(),
                this.loadRules()
            ]);
            
            
            // Add vehicle markers to map after data is loaded
            if (this.map) {
                this.addVehicleMarkers();
            }
        } catch (error) {
            console.error('❌ Error loading fleet data:', error);
            this.showNotification('Failed to load fleet data', 'error');
        }
    }
    
    async loadVehicles() {
        try {
            const response = await fetch(`${this.apiBaseUrl}/vehicles`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $v isa vehicle, has vehicle-id $id, has vehicle-type $type, has status $status, has vehicle-icon $icon, has status-color $color, has gps-latitude $lat, has gps-longitude $lng, has capacity $capacity, has fuel-level $fuel, has maintenance-score $maintenance; $assignment isa assignment (assigned-vehicle: $v, assigned-employee: $employee); select $v, $id, $type, $status, $icon, $color, $lat, $lng, $capacity, $fuel, $maintenance; limit 50;'
                })
            });
            const result = await response.json();
            this.data.vehicles = result.ok?.answers;
        } catch (error) {
            console.error('Error loading vehicles:', error);
            this.data.vehicles = [];
        }
    }
    
    async loadDrivers() {
        try {
            const response = await fetch(`${this.apiBaseUrl}/employees`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $e isa employee, has employee-role "driver"; select $e; limit 100;'
                })
            });
            const result = await response.json();
            this.data.drivers = result.ok?.answers;
        } catch (error) {
            console.error('Error loading drivers:', error);
            this.data.drivers = [];
        }
    }
    
    async loadDeliveries() {
        try {
            const response = await fetch(`${this.apiBaseUrl}/deliveries`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $d isa delivery; select $d; limit 100;'
                })
            });
            const result = await response.json();
            this.data.deliveries = result.ok?.answers;
        } catch (error) {
            console.error('Error loading deliveries:', error);
            this.data.deliveries = [];
        }
    }
    
    async loadOperations() {
        try {
            const response = await fetch(`${this.apiBaseUrl}/operations`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $op isa operation; select $op; limit 50;'
                })
            });
            const result = await response.json();
            this.data.operations = result.ok?.answers;
        } catch (error) {
            console.error('Error loading operations:', error);
            this.data.operations = [];
        }
    }
    
    async loadJobs() {
        try {
            const response = await fetch(`${this.apiBaseUrl}/jobs`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'stats'
                },
                body: JSON.stringify({})
            });
            const result = await response.json();
            this.data.jobs = result;
        } catch (error) {
            console.error('Error loading jobs:', error);
            this.data.jobs = {};
        }
    }
    
    async loadRules() {
        try {
            const response = await fetch(`${this.apiBaseUrl}/rules`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $r isa rule; select $r; limit 50;'
                })
            });
            const result = await response.json();
            this.data.rules = result.ok?.answers;
        } catch (error) {
            console.error('Error loading rules:', error);
            this.data.rules = [];
        }
    }
    
    initializeMap() {
        
        if (!tt) {
            console.error('TomTom SDK not loaded');
            return;
        }
        
        this.map = tt.map({
            key: this.tomtomApiKey,
            container: 'mapView',
            zoom: 11,
            style: {
                map: 'basic_main',
                poi: 'poi_main'
            },
            stylesVisibility: {
                trafficIncidents: true,
                trafficFlow: true
            }
        });
        
        // Don't add default navigation controls since we have custom ones
        // this.map.addControl(new tt.NavigationControl());
        
        this.map.on('ready', async () => {
            // Add custom CSS for animations
            const style = document.createElement('style');
            style.textContent = `
                @keyframes pulse {
                    0% { opacity: 1; transform: scale(1); }
                    50% { opacity: 0.7; transform: scale(1.1); }
                    100% { opacity: 1; transform: scale(1); }
                }
                .vehicle-marker {
                    z-index: 100;
                }
                .vehicle-marker:hover {
                    z-index: 1000 !important;
                }
                .traffic-control {
                    position: absolute;
                    bottom: 20px;
                    right: 20px;
                    background: rgba(255, 255, 255, 0.95);
                    backdrop-filter: blur(10px);
                    border-radius: 8px;
                    padding: 12px;
                    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
                    z-index: 1000;
                    border: 1px solid rgba(226, 232, 240, 0.8);
                }
                @media (max-width: 768px) {
                    .traffic-control {
                        right: 10px;
                        bottom: 20px;
                        font-size: 11px;
                        padding: 8px;
                    }
                    .incident-alert {
                        right: 10px !important;
                        top: 120px !important;
                        max-width: calc(100vw - 20px) !important;
                    }
                }
                .incident-alert {
                    position: absolute;
                    top: 80px;
                    right: 420px;
                    background: linear-gradient(135deg, #ef4444, #dc2626);
                    color: white;
                    border-radius: 8px;
                    padding: 12px;
                    box-shadow: 0 4px 12px rgba(239, 68, 68, 0.4);
                    z-index: 1001;
                    max-width: 280px;
                    animation: slideIn 0.3s ease-out;
                    border: 1px solid rgba(255, 255, 255, 0.2);
                }
                @keyframes slideIn {
                    from { transform: translateX(100%); opacity: 0; }
                    to { transform: translateX(0); opacity: 1; }
                }
  /* --- TomTom popup: remove inner padding + clip overflow --- */
  .custom-popup .tt-popup-content {
    padding: 0 !important;
    border-radius: 16px;
    overflow: hidden;
    width: auto !important;
    max-width: none !important;
    min-width: auto !important;
  }

  /* --- TomTom popup wrapper constraints --- */
  .custom-popup .tt-popup {
    width: auto !important;
    max-width: none !important;
  }

  /* --- Force TomTom to respect our container width --- */
  .custom-popup {
    width: auto !important;
    max-width: none !important;
  }

  /* --- Fix TomTom close button positioning --- */
  .custom-popup .tt-popup-close-button {
    position: absolute !important;
    top: 8px !important;
    right: 8px !important;
    z-index: 1000 !important;
    background: rgba(255, 255, 255, 0.9) !important;
    border-radius: 50% !important;
    width: 24px !important;
    height: 24px !important;
    display: flex !important;
    align-items: center !important;
    justify-content: center !important;
    font-size: 16px !important;
    color: #64748b !important;
    border: 1px solid rgba(226, 232, 240, 0.8) !important;
    cursor: pointer !important;
  }

  .custom-popup .tt-popup-close-button:hover {
    background: rgba(248, 250, 252, 1) !important;
    color: #334155 !important;
  }

  /* --- Your popup container: keep sizing sane --- */
  .vehicle-popup-container {
    box-sizing: border-box;
    max-width: 90vw;
    overflow: hidden;
    word-wrap: break-word;
    overflow-wrap: break-word;
    padding: 24px !important; /* Increased padding for better whitespace */
  }

  /* --- Ensure all popup content respects container width --- */
  .vehicle-popup-container * {
    box-sizing: border-box;
    max-width: 100%;
  }

  /* --- Fix text overflow in popup sections --- */
  .vehicle-popup-container .text-sm,
  .vehicle-popup-container .text-xs,
  .vehicle-popup-container .font-medium {
    word-wrap: break-word;
    overflow-wrap: break-word;
    white-space: normal;
  }

  /* --- GPS coordinates and long text should wrap --- */
  .vehicle-popup-container .text-slate-600 {
    word-wrap: break-word;
    overflow-wrap: break-word;
    white-space: normal;
    max-width: 100%;
  }

  /* --- Fix grid layouts that cause overflow --- */
  .vehicle-popup-container .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 20px; /* Increased gap for better spacing */
    width: 100%;
    min-width: 0; /* Allow grid items to shrink */
  }

  .vehicle-popup-container .grid > div {
    min-width: 0; /* Allow grid items to shrink */
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 12px 0; /* Add vertical padding to grid items */
  }

  /* --- Specific fix for grid-cols-2 class --- */
  .vehicle-popup-container .grid-cols-2 {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 24px; /* Generous gap for better visual separation */
  }

  /* --- Better spacing for grid-cols-3 --- */
  .vehicle-popup-container .grid-cols-3 {
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 20px;
  }

  /* --- Force content to fit within bounds --- */
  .vehicle-popup-container .metric-value {
    font-size: 18px !important;
    line-height: 1.3 !important;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-top: 8px !important;
    font-weight: 600 !important;
  }

  .vehicle-popup-container .metric-label {
    font-size: 13px !important;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #64748b !important;
    font-weight: 500 !important;
    text-transform: uppercase !important;
    letter-spacing: 0.05em !important;
  }

  /* --- Constrain flex layouts more aggressively --- */
  .vehicle-popup-container .flex {
    flex-wrap: wrap;
    min-width: 0;
    overflow: hidden;
  }

  .vehicle-popup-container .flex-1 {
    min-width: 0;
    flex-shrink: 1;
    overflow: hidden;
  }

  /* --- Responsive grid for small popups --- */
  @media (max-width: 480px) {
    .vehicle-popup-container .grid {
      grid-template-columns: 1fr;
      gap: 12px;
    }
  }

  /* --- Fix flex layouts --- */
  .vehicle-popup-container .flex {
    flex-wrap: wrap;
    min-width: 0;
  }

  .vehicle-popup-container .flex > * {
    min-width: 0;
    flex-shrink: 1;
  }

  /* --- Footer: DO NOT overflow. Wrap/stack buttons if needed --- */
  .vehicle-popup-footer {
    display: flex;
    gap: 16px; /* Increased gap between buttons */
    flex-wrap: wrap;
    width: 100%;
    margin-top: 32px; /* More space above footer */
    padding-top: 24px; /* Padding above border */
  }

  .vehicle-popup-footer button {
    flex: 1 1 160px; /* grows, shrinks, min-ish width */
    min-width: 160px;
    white-space: nowrap;
    padding: 12px 24px; /* More generous button padding */
  }

  /* On small screens, stack buttons */
  @media (max-width: 480px) {
    .vehicle-popup-footer button {
      flex-basis: 100%;
      min-width: 100%;
    }
  }
            `;
            document.head.appendChild(style);
            
            this.addTrafficControls();
            await this.addVehicleMarkers();
            this.startTrafficMonitoring();
        });
    }
    
    async addVehicleMarkers() {
        if (!this.map) return;
        
        
        // Clear existing markers
        this.vehicleMarkers.forEach(marker => marker.remove());
        this.vehicleMarkers.clear();
        
        if (!this.data.vehicles || this.data.vehicles.length === 0) {
            return;
        }
        
        // Process vehicles sequentially to ensure proper async handling
        const vehiclePromises = this.data.vehicles.map(async (vehicle, index) => {
            // Extract data from the new query structure
            const vehicleData = vehicle.data || {};
            
            
            // Extract GPS coordinates from database
            const lat = vehicleData.lat ? parseFloat(vehicleData.lat.value) : null;
            const lng = vehicleData.lng ? parseFloat(vehicleData.lng.value) : null;
            
            
            // Skip vehicles without GPS coordinates from database
            if (!lat || !lng) {
                return;
            }
            
            // Extract required fields from database
            const vehicleId = vehicleData.id?.value;
            const status = vehicleData.status?.value;
            const vehicleType = vehicleData.type?.value;
            const vehicleIcon = vehicleData.icon?.value;
            const statusColor = vehicleData.color?.value;
            const capacity = vehicleData.capacity?.value;
            const fuelLevel = vehicleData.fuel?.value;
            const maintenanceScore = vehicleData.maintenance?.value;
            // Get real driver data from database
            const driverData = await this.getDriverForVehicle(vehicleId);
            const driverName = driverData?.name || 'Unassigned';
            const driverStatus = driverData?.status || 'unknown';
            const driverRating = driverData?.rating || 0;
            const driverCerts = driverData?.certifications || 'N/A';
            
            
            // Skip vehicles without required database fields
            if (!vehicleId || !status || !vehicleType || !vehicleIcon || !statusColor) {
                return;
            }
            
            
            const markerElement = document.createElement('div');
            markerElement.className = 'vehicle-marker';
            
            markerElement.innerHTML = `
                <div style="
                    width: 32px; 
                    height: 32px; 
                    background: linear-gradient(135deg, ${statusColor}, ${this.darkenColor(statusColor, 0.2)});
                    border: 3px solid #ffffff;
                    border-radius: 10px;
                    display: flex;
                    flex-direction: column;
                    align-items: center;
                    justify-content: center;
                    box-shadow: 0 8px 20px rgba(0,0,0,0.8), 0 0 0 2px rgba(0,0,0,0.4), 0 0 15px rgba(0,0,0,0.5);
                    cursor: pointer;
                    position: relative;
                    font-size: 16px;
                    transition: all 0.3s ease;
                    backdrop-filter: blur(10px);
                    outline: 1px solid rgba(0,0,0,0.2);
                    outline-offset: 1px;
                " onmouseover="this.style.transform='scale(1.3)'; this.style.zIndex='1000'; this.style.boxShadow='0 12px 30px rgba(0,0,0,0.9), 0 0 0 3px rgba(0,0,0,0.5), 0 0 25px rgba(0,0,0,0.6)';" onmouseout="this.style.transform='scale(1)'; this.style.zIndex='auto'; this.style.boxShadow='0 8px 20px rgba(0,0,0,0.8), 0 0 0 2px rgba(0,0,0,0.4), 0 0 15px rgba(0,0,0,0.5)';">
                    <div style="font-size: 14px; line-height: 1; filter: drop-shadow(0 2px 4px rgba(0,0,0,0.7)) contrast(1.3);">${vehicleIcon}</div>
                    <div style="
                        position: absolute;
                        top: -4px;
                        right: -4px;
                        width: 12px;
                        height: 12px;
                        background: radial-gradient(circle, ${statusColor}, ${this.darkenColor(statusColor, 0.3)});
                        border: 2px solid #ffffff;
                        border-radius: 50%;
                        box-shadow: 0 3px 8px rgba(0,0,0,0.7), 0 0 0 1px rgba(0,0,0,0.3);
                        animation: ${status === 'operational' || status === 'busy' ? 'pulse 2s infinite' : 'none'};
                    "></div>
                </div>
                <div style="
                    position: absolute;
                    top: 36px;
                    left: 50%;
                    transform: translateX(-50%);
                    background: linear-gradient(135deg, rgba(0,0,0,0.95), rgba(0,0,0,0.8));
                    color: #ffffff;
                    padding: 2px 6px;
                    border-radius: 4px;
                    font-size: 9px;
                    font-weight: 800;
                    white-space: nowrap;
                    box-shadow: 0 4px 12px rgba(0,0,0,0.8), 0 0 0 1px rgba(255,255,255,0.2);
                    border: 1px solid rgba(255,255,255,0.4);
                    backdrop-filter: blur(5px);
                    text-shadow: 0 1px 2px rgba(0,0,0,0.9);
                ">${vehicleId}</div>
            `;
            
            const marker = new tt.Marker({ element: markerElement })
                .setLngLat([lng, lat])
                .addTo(this.map);
            
            const popup = new tt.Popup({ 
                offset: 35,
                className: 'custom-popup',
                closeOnClick: false,
                closeButton: false,
                maxWidth: 'none'
            })
                .setHTML(`
                    <div id="popup-${vehicleId}" class="vehicle-popup-container transition-all duration-300" style="width: 420px; max-width: 90vw;">
                        <!-- Header Section -->
                        <div class="flex items-start justify-between mb-6">
                            <div class="flex items-center gap-3">
                                <div class="metric-icon" style="background-color: ${statusColor}; width: 24px; height: 24px; display: flex; align-items: center; justify-content: center; border-radius: 6px;">
                                    <span class="text-white text-sm">${vehicleIcon}</span>
                                </div>
                                <div>
                                    <h3 class="text-base font-semibold text-slate-900 mb-1">${vehicleId}</h3>
                                    <p class="text-xs text-slate-600 capitalize">${vehicleType.replace('_', ' ')}</p>
                                </div>
                            </div>
                            
                            <div class="bg-slate-50 rounded-lg p-4 min-w-0 flex-1 ml-4">
                                <h3 class="text-base font-semibold text-slate-900 mb-3">Driver Information</h3>
                                <div class="space-y-3">
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Driver:</span>
                                        <span class="text-slate-900 font-medium text-sm">${driverName}</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Status:</span>
                                        <span class="px-2 py-1 text-xs font-medium rounded-full capitalize text-white" style="background: ${statusColor};">${driverStatus}</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Rating:</span>
                                        <span class="text-slate-900 font-medium text-sm">${driverRating}/5 ⭐</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Certifications:</span>
                                        <span class="text-slate-900 font-medium text-sm">${driverCerts}</span>
                                    </div>
                                    
                                    <div class="pt-3 border-t border-gray-100">
                                        <div class="flex gap-2">
                                            <button onclick="fleetCommand.showCallPanel('${vehicleId}', '${driverName}')" class="flex-1 text-xs px-3 py-2 bg-gray-100 hover:bg-gray-200 border border-gray-300 rounded text-gray-700 transition-colors">
                                                📞 Call
                                            </button>
                                            <button onclick="fleetCommand.showMessagePanel('${vehicleId}', '${driverName}')" class="flex-1 text-xs px-3 py-2 bg-blue-500 hover:bg-blue-600 border border-blue-500 rounded text-white transition-colors">
                                                💬 Message
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            </div>
                            
                            <div id="comm-${vehicleId}" class="hidden ml-6 pl-6 border-l border-gray-200" style="width: 240px;">
                                <div id="call-panel-${vehicleId}" class="hidden">
                                    <div class="text-center py-4">
                                        <div style="width: 20px; height: 20px; display: flex; align-items: center; justify-content: center; border-radius: 6px; background: #dcfce7; margin: 0 auto 12px;">
                                            <span class="text-green-600 text-sm">📞</span>
                                        </div>
                                        <div class="text-sm font-medium mb-2">Calling ${driverName}</div>
                                        <div class="text-xs text-slate-600 mb-4">Vehicle: ${vehicleId}</div>
                                        <button onclick="fleetCommand.endCall('${vehicleId}')" class="text-xs px-3 py-2 bg-red-500 hover:bg-red-600 border border-red-500 rounded text-white transition-colors">
                                            End Call
                                        </button>
                                    </div>
                                </div>
                                
                                <div id="message-panel-${vehicleId}" class="hidden space-y-4">
                                    <div class="text-sm font-medium mb-3">Message ${driverName}</div>
                                    <div class="space-y-2">
                                        <button onclick="fleetCommand.sendQuickMessage('${vehicleId}', '${driverName}', 'Please update your status')" class="w-full text-left px-3 py-2 text-xs bg-gray-50 hover:bg-gray-100 rounded border transition-colors">
                                            📍 Update status
                                        </button>
                                        <button onclick="fleetCommand.sendQuickMessage('${vehicleId}', '${driverName}', 'Return to depot')" class="w-full text-left px-3 py-2 text-xs bg-gray-50 hover:bg-gray-100 rounded border transition-colors">
                                            🏠 Return to depot
                                        </button>
                                        <button onclick="fleetCommand.sendQuickMessage('${vehicleId}', '${driverName}', 'Take your break')" class="w-full text-left px-3 py-2 text-xs bg-gray-50 hover:bg-gray-100 rounded border transition-colors">
                                            ☕ Take break
                                        </button>
                                    </div>
                                    
                                    <textarea id="custom-msg-${vehicleId}" class="w-full px-3 py-2 text-xs border border-gray-200 rounded focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" rows="2" placeholder="Custom message..."></textarea>
                                    
                                    <div class="flex gap-2">
                                        <button onclick="fleetCommand.sendCustomMessage('${vehicleId}', '${driverName}')" class="flex-1 text-xs px-3 py-2 bg-blue-500 hover:bg-blue-600 border border-blue-500 rounded text-white transition-colors">
                                            Send
                                        </button>
                                        <button onclick="fleetCommand.closeCommPanel('${vehicleId}')" class="text-xs px-3 py-2 bg-gray-100 hover:bg-gray-200 border border-gray-300 rounded text-gray-700 transition-colors">
                                            ✕
                                        </button>
                                    </div>
                                </div>
                            </div>
                        </div>
                        
                        <!-- Details Section (initially hidden) -->
                        <div id="details-${vehicleId}" class="hidden mt-6 pt-6 border-t border-gray-100">
                            <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
                                <div class="bg-slate-50 rounded-lg p-4">
                                    <h3 class="text-base font-semibold text-slate-900 mb-4">Status & Location</h3>
                                    <div class="space-y-3">
                                        <div class="flex justify-between items-center">
                                            <span class="text-slate-600 text-sm">Status:</span>
                                            <span class="px-2 py-1 text-xs font-medium rounded-full capitalize text-white" style="background: ${statusColor};">${status}</span>
                                        </div>
                                        <div class="flex justify-between items-center">
                                            <span class="text-slate-600 text-sm">GPS Location:</span>
                                            <span class="text-slate-900 font-mono text-xs">${parseFloat(lat)?.toFixed(4)}, ${parseFloat(lng)?.toFixed(4)}</span>
                                        </div>
                                        <div class="flex justify-between items-center">
                                            <span class="text-slate-600 text-sm">Type:</span>
                                            <span class="text-slate-900 font-medium text-sm">${vehicleType.replace('_', ' ')}</span>
                                        </div>
                                    </div>
                                </div>
                                
                                <div class="bg-slate-50 rounded-lg p-4">
                                    <h3 class="text-base font-semibold text-slate-900 mb-4">Performance Metrics</h3>
                                    <div class="space-y-4">
                                        <div>
                                            <div class="flex justify-between items-center mb-2">
                                                <span class="text-slate-600 text-sm">Fuel Level</span>
                                                <span class="text-slate-900 font-medium text-sm">${fuelLevel}%</span>
                                            </div>
                                            <div class="w-full h-2 bg-slate-200 rounded-full overflow-hidden">
                                                <div class="h-full rounded-full transition-all" style="width: ${fuelLevel}%; background: ${fuelLevel > 50 ? '#22c55e' : fuelLevel > 25 ? '#f59e0b' : '#ef4444'};"></div>
                                            </div>
                                        </div>
                                        
                                        <div>
                                            <div class="flex justify-between items-center mb-2">
                                                <span class="text-slate-600 text-sm">Maintenance Score</span>
                                                <span class="text-slate-900 font-medium text-sm">${maintenanceScore}/100</span>
                                            </div>
                                            <div class="w-full h-2 bg-slate-200 rounded-full overflow-hidden">
                                                <div class="h-full rounded-full transition-all" style="width: ${maintenanceScore}%; background: ${maintenanceScore > 70 ? '#22c55e' : maintenanceScore > 40 ? '#f59e0b' : '#ef4444'};"></div>
                                            </div>
                                        </div>
                                        
                                        <div class="flex justify-between items-center">
                                            <span class="text-slate-600 text-sm">Capacity:</span>
                                            <span class="text-slate-900 font-medium text-sm">${capacity} kg</span>
                                        </div>
                                    </div>
                                </div>
                            </div>
                            
                            <!-- Route & Traffic Data with proper spacing -->
                            <div id="route-info-${vehicleId}" class="bg-blue-50 rounded-lg p-4 mt-6">
                                <h3 class="text-base font-semibold text-slate-900 mb-4">Route & Traffic</h3>
                                <div class="space-y-3">
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Current Route:</span>
                                        <span class="text-slate-900 font-medium text-sm" id="route-status-${vehicleId}">Loading...</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Traffic Conditions:</span>
                                        <span class="text-slate-900 font-medium text-sm" id="traffic-status-${vehicleId}">Checking...</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">ETA to Destination:</span>
                                        <span class="text-slate-900 font-medium text-sm" id="eta-${vehicleId}">Calculating...</span>
                                    </div>
                                </div>
                            </div>
                        </div>
                        
                        <!-- Footer Actions -->
                        <div class="vehicle-popup-footer mt-6 pt-4 border-t border-gray-100">
                            <div class="flex gap-3">
                                <button onclick="fleetCommand.trackVehicle('${vehicleId}')" class="text-xs px-4 py-2 bg-blue-500 hover:bg-blue-600 border border-blue-500 rounded text-white transition-colors">
                                    Track Vehicle
                                </button>
                                <button onclick="fleetCommand.toggleVehicleDetails('${vehicleId}')" class="text-xs px-4 py-2 bg-gray-100 hover:bg-gray-200 border border-gray-300 rounded text-gray-700 transition-colors">
                                    <span id="details-btn-${vehicleId}">Show Details</span>
                                </button>
                            </div>
                        </div>
                    </div>
                `);
            
            marker.setPopup(popup);
            this.vehicleMarkers.set(vehicleId, marker);
            
            // Add popup open event listener
            popup.on('open', () => {
                // Load traffic data when popup opens
                setTimeout(() => this.loadVehicleTrafficData(vehicleId, lat, lng), 100);
            });
            
            // Add intelligent popup positioning and map centering
            marker.on('click', () => {
                // Center the map on the selected vehicle
                this.centerMapOnVehicle(vehicleId, [lng, lat]);
                // Then ensure popup is in view
                setTimeout(() => this.ensurePopupInView(vehicleId, [lng, lat]), 300);
                // Load traffic data for this vehicle
                setTimeout(() => this.loadVehicleTrafficData(vehicleId, lat, lng), 500);
            });
        });
        
        // Wait for all vehicle markers to be processed
        await Promise.all(vehiclePromises);
        
        
        // Center map on all vehicles after all markers are added
        this.centerMapOnVehicles();
    }
    
    ensurePopupInView(vehicleId, coordinates) {
        if (!this.map) return;
        
        const popup = document.querySelector(`#popup-${vehicleId}`);
        if (!popup) return;
        
        // Get popup dimensions
        const popupRect = popup.getBoundingClientRect();
        const mapContainer = this.map.getContainer();
        const mapRect = mapContainer.getBoundingClientRect();
        
        // Calculate viewport boundaries (accounting for sidebar)
        const sidebarWidth = 400; // Approximate sidebar width
        const viewportWidth = window.innerWidth - sidebarWidth;
        const viewportHeight = window.innerHeight;
        
        // Check if popup is off-screen
        const isOffRight = popupRect.right > viewportWidth;
        const isOffLeft = popupRect.left < sidebarWidth;
        const isOffBottom = popupRect.bottom > viewportHeight;
        const isOffTop = popupRect.top < 0;
        
        if (isOffRight || isOffLeft || isOffBottom || isOffTop) {
            
            // Calculate optimal map center to keep popup in view
            const [lng, lat] = coordinates;
            const currentCenter = this.map.getCenter();
            
            let newLng = currentCenter.lng;
            let newLat = currentCenter.lat;
            
            // Adjust horizontal position
            if (isOffRight) {
                // Move map left to bring popup into view
                const offsetDegrees = this.pixelsToLngLat(popupRect.width / 2 + 50);
                newLng = lng - offsetDegrees.lng;
            } else if (isOffLeft) {
                // Move map right to bring popup into view  
                const offsetDegrees = this.pixelsToLngLat(popupRect.width / 2 + sidebarWidth + 50);
                newLng = lng + offsetDegrees.lng;
            }
            
            // Adjust vertical position
            if (isOffBottom) {
                // Move map up to bring popup into view
                const offsetDegrees = this.pixelsToLngLat(popupRect.height / 2 + 50);
                newLat = lat + offsetDegrees.lat;
            } else if (isOffTop) {
                // Move map down to bring popup into view
                const offsetDegrees = this.pixelsToLngLat(popupRect.height / 2 + 50);
                newLat = lat - offsetDegrees.lat;
            }
            
            // Smoothly pan to new position
            this.map.easeTo({
                center: [newLng, newLat],
                duration: 500,
                easing: (t) => t * (2 - t) // easeOutQuad
            });
        }
    }

    ensurePopupInViewGentle(vehicleId, coordinates) {
        if (!this.map) return;
        
        const popup = document.querySelector(`#popup-${vehicleId}`);
        if (!popup) return;
        
        // Get popup dimensions
        const popupRect = popup.getBoundingClientRect();
        const viewportWidth = window.innerWidth - 400; // Account for sidebar
        const viewportHeight = window.innerHeight;
        
        // Only adjust if popup is COMPLETELY off-screen (more than 80% hidden)
        const visibleWidth = Math.max(0, Math.min(popupRect.right, viewportWidth) - Math.max(popupRect.left, 400));
        const visibleHeight = Math.max(0, Math.min(popupRect.bottom, viewportHeight) - Math.max(popupRect.top, 0));
        const visibleArea = visibleWidth * visibleHeight;
        const totalArea = popupRect.width * popupRect.height;
        const visibilityRatio = totalArea > 0 ? visibleArea / totalArea : 0;
        
        // Only move map if less than 20% of popup is visible
        if (visibilityRatio < 0.2) {
            
            const [lng, lat] = coordinates;
            const currentCenter = this.map.getCenter();
            
            // Make minimal adjustment - just enough to show more of the popup
            const adjustmentFactor = 0.3; // Reduce movement by 70%
            let newLng = currentCenter.lng;
            let newLat = currentCenter.lat;
            
            // Gentle horizontal adjustment
            if (popupRect.right > viewportWidth) {
                const offsetDegrees = this.pixelsToLngLat(100); // Fixed small offset
                newLng = currentCenter.lng - offsetDegrees.lng * adjustmentFactor;
            } else if (popupRect.left < 400) {
                const offsetDegrees = this.pixelsToLngLat(100);
                newLng = currentCenter.lng + offsetDegrees.lng * adjustmentFactor;
            }
            
            // Gentle vertical adjustment
            if (popupRect.bottom > viewportHeight) {
                const offsetDegrees = this.pixelsToLngLat(100);
                newLat = currentCenter.lat + offsetDegrees.lat * adjustmentFactor;
            } else if (popupRect.top < 0) {
                const offsetDegrees = this.pixelsToLngLat(100);
                newLat = currentCenter.lat - offsetDegrees.lat * adjustmentFactor;
            }
            
            // Gentle pan with shorter duration
            this.map.easeTo({
                center: [newLng, newLat],
                duration: 300,
                easing: (t) => t * (2 - t)
            });
        }
    }
    
    pixelsToLngLat(pixels) {
        // Convert pixel distance to approximate lng/lat degrees
        // This is a rough approximation - more precise calculation would use map projection
        const zoom = this.map.getZoom();
        const scale = Math.pow(2, zoom);
        const degreePerPixel = 360 / (256 * scale);
        
        return {
            lng: pixels * degreePerPixel,
            lat: pixels * degreePerPixel * Math.cos(this.map.getCenter().lat * Math.PI / 180)
        };
    }

    darkenColor(color, amount) {
        // Convert hex to RGB, darken, and convert back
        const hex = color.replace('#', '');
        const r = Math.max(0, parseInt(hex.substr(0, 2), 16) * (1 - amount));
        const g = Math.max(0, parseInt(hex.substr(2, 2), 16) * (1 - amount));
        const b = Math.max(0, parseInt(hex.substr(4, 2), 16) * (1 - amount));
        return `#${Math.round(r).toString(16).padStart(2, '0')}${Math.round(g).toString(16).padStart(2, '0')}${Math.round(b).toString(16).padStart(2, '0')}`;
    }
    
    centerMapOnVehicle(vehicleId, coordinates) {
        if (!this.map) {
            return;
        }
        
        const [lng, lat] = coordinates;
        
        // Smoothly pan and zoom to the selected vehicle
        this.map.easeTo({
            center: [lng, lat],
            zoom: Math.max(this.map.getZoom(), 13), // Ensure minimum zoom level for detail
            duration: 800,
            easing: (t) => t * (2 - t) // easeOutQuad for smooth animation
        });
        
        // Show notification
        this.showNotification(`Centered on vehicle ${vehicleId}`, 'info');
    }

    centerMapOnVehicles() {
        
        if (!this.map || this.vehicleMarkers.size === 0) {
            return;
        }
        
        const bounds = new tt.LngLatBounds();
        let hasValidBounds = false;
        
        // Extend bounds to include all vehicle markers
        this.vehicleMarkers.forEach((marker, vehicleId) => {
            const lngLat = marker.getLngLat();
            if (lngLat && lngLat.lng && lngLat.lat) {
                bounds.extend(lngLat);
                hasValidBounds = true;
            }
        });
        
        
        if (hasValidBounds) {
            // Add padding around the bounds
            const padding = { top: 80, bottom: 80, left: 80, right: 400 }; // Extra right padding for sidebar
            
            
            this.map.fitBounds(bounds, {
                padding: padding,
                maxZoom: 14, // Don't zoom in too close
                duration: 1000 // Smooth animation
            });
            
        } else {
        }
    }

    addTrafficControls() {
        const mapContainer = document.getElementById('mapView');
        if (!mapContainer) return;

        const trafficControl = document.createElement('div');
        trafficControl.className = 'traffic-control';
        trafficControl.innerHTML = `
            <div class="flex flex-col space-y-2">
                <div class="text-xs font-semibold text-slate-800 mb-1">Traffic Monitoring</div>
                <div class="flex items-center space-x-3">
                    <label class="flex items-center space-x-1 text-xs font-medium text-slate-700 cursor-pointer">
                        <input type="checkbox" id="trafficFlow" checked class="w-3 h-3 rounded border-slate-300 text-blue-600 focus:ring-blue-500">
                        <span>Flow</span>
                    </label>
                    <label class="flex items-center space-x-1 text-xs font-medium text-slate-700 cursor-pointer">
                        <input type="checkbox" id="trafficIncidents" checked class="w-3 h-3 rounded border-slate-300 text-blue-600 focus:ring-blue-500">
                        <span>Incidents</span>
                    </label>
                    <button id="refreshTraffic" class="px-3 py-1 bg-blue-600 text-white text-xs rounded-md hover:bg-blue-700 transition-all duration-200 font-medium shadow-sm">
                        🔄
                    </button>
                </div>
            </div>
        `;

        mapContainer.appendChild(trafficControl);

        // Add event listeners
        document.getElementById('trafficFlow').addEventListener('change', (e) => {
            this.toggleTrafficLayer('trafficFlow', e.target.checked);
        });

        document.getElementById('trafficIncidents').addEventListener('change', (e) => {
            this.toggleTrafficLayer('trafficIncidents', e.target.checked);
        });

        document.getElementById('refreshTraffic').addEventListener('click', () => {
            this.refreshTrafficData();
        });
    }

    toggleTrafficLayer(layer, enabled) {
        if (!this.map) return;
        
        try {
            if (layer === 'trafficFlow') {
                // Use TomTom's native traffic layer functionality
                const currentStyle = this.map.getStyle();
                const newStylesVisibility = {
                    ...currentStyle.stylesVisibility,
                    trafficFlow: enabled
                };
                
                // Update the map style with traffic flow visibility
                this.map.setStyle({
                    ...currentStyle,
                    stylesVisibility: newStylesVisibility
                });
                
            } else if (layer === 'trafficIncidents') {
                // Use TomTom's native incidents layer functionality  
                const currentStyle = this.map.getStyle();
                const newStylesVisibility = {
                    ...currentStyle.stylesVisibility,
                    trafficIncidents: enabled
                };
                
                // Update the map style with traffic incidents visibility
                this.map.setStyle({
                    ...currentStyle,
                    stylesVisibility: newStylesVisibility
                });
            }
        } catch (error) {
            console.error(`Error toggling traffic layer ${layer}:`, error);
            // Fallback: just show notification
            this.showNotification(`Traffic ${layer} toggle failed`, 'error');
        }
    }

    async startTrafficMonitoring() {
        
        // Monitor traffic every 2 minutes
        setInterval(() => {
            this.checkTrafficConditions();
        }, 120000);

        // Initial traffic check
        this.checkTrafficConditions();
    }

    async checkTrafficConditions() {
        try {
            // Get current vehicle positions and check traffic for active routes
            const activeVehicles = this.data.vehicles.filter(v => 
                v.data?.vehicle?.status === 'operational' || v.data?.vehicle?.status === 'busy'
            );

            for (const vehicle of activeVehicles.slice(0, 3)) { // Limit to 3 vehicles to avoid API rate limits
                const vehicleData = vehicle.data?.vehicle || {};
                const vehicleId = vehicleData['vehicle-id'] || 'unknown';
                
                // Traffic checking disabled - should use real vehicle GPS coordinates
                // TODO: Integrate with real vehicle GPS data when available
            }
        } catch (error) {
            console.error('Traffic monitoring error:', error);
        }
    }

    async loadVehicleTrafficData(vehicleId, lat, lng) {
        try {
            // Check if DOM elements exist first
            const routeStatus = document.getElementById(`route-status-${vehicleId}`);
            const trafficStatus = document.getElementById(`traffic-status-${vehicleId}`);
            const etaElement = document.getElementById(`eta-${vehicleId}`);
            
            if (!routeStatus || !trafficStatus || !etaElement) {
                return;
            }
            
            // Update UI to show loading state
            routeStatus.textContent = 'Loading...';
            trafficStatus.textContent = 'Loading...';
            etaElement.textContent = 'Calculating...';
            
            // Get route data from database for this vehicle
            const response = await fetch(`${this.apiBaseUrl}/deliveries`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `
                        match 
                        $assignment isa assignment (assigned-vehicle: $vehicle, assigned-delivery: $delivery);
                        $vehicle has vehicle-id "${vehicleId}";
                        $delivery has route-id $routeId,
                                 has pickup-address $pickup,
                                 has delivery-address $destination,
                                 has delivery-time $deliveryTime,
                                 has status $deliveryStatus;
                        select $routeId, $pickup, $destination, $deliveryTime, $deliveryStatus;
                    `
                })
            });
            
            const routeData = await response.json();
            
            if (routeData.ok && routeData.ok.answers && routeData.ok.answers.length > 0) {
                const route = routeData.ok.answers[0].data;
                const routeId = route.routeId?.value || 'Unknown';
                const destination = route.destination?.value || 'Unknown destination';
                const deliveryTime = route.deliveryTime?.value || '';
                const deliveryStatus = route.deliveryStatus?.value || 'pending';
                
                // Calculate ETA based on delivery time
                let eta = 'Unknown';
                if (deliveryTime) {
                    const deliveryDate = new Date(deliveryTime);
                    const now = new Date();
                    const diffMinutes = Math.round((deliveryDate - now) / (1000 * 60));
                    
                    if (diffMinutes > 0) {
                        eta = `${Math.round(diffMinutes / 60)}h ${diffMinutes % 60}m`;
                    } else {
                        eta = deliveryStatus === 'delivered' ? 'Delivered' : 'Overdue';
                    }
                }
                
                // Get traffic conditions based on route and destination
                const trafficConditions = this.getTrafficConditionForRoute(routeId, destination);
                
                // Update UI with database route data
                routeStatus.textContent = `${routeId} → ${destination.split(',')[0]}`;
                trafficStatus.innerHTML = `<span style="color: ${trafficConditions.color};">${trafficConditions.condition}</span>`;
                etaElement.textContent = eta;
                
            } else {
                routeStatus.textContent = 'No Active Route';
                trafficStatus.innerHTML = '<span style="color: #6b7280;">Unknown</span>';
                etaElement.textContent = 'N/A';
            }
            
        } catch (error) {
            const routeStatus = document.getElementById(`route-status-${vehicleId}`);
            const trafficStatus = document.getElementById(`traffic-status-${vehicleId}`);
            const etaElement = document.getElementById(`eta-${vehicleId}`);
            
            if (routeStatus) routeStatus.textContent = 'Error Loading Route';
            if (trafficStatus) trafficStatus.innerHTML = '<span style="color: #ef4444;">Error</span>';
            if (etaElement) etaElement.textContent = 'Error';
        }
    }
    
    getTrafficConditionForRoute(routeId, destination) {
        // Simulate realistic traffic conditions based on route and destination
        const manhattanRoutes = ['RT001', 'RT002'];
        const brooklynRoutes = ['RT003', 'RT004'];
        
        if (destination.includes('Manhattan') || manhattanRoutes.includes(routeId)) {
            return { condition: 'Heavy Traffic', color: '#ef4444' };
        } else if (destination.includes('Brooklyn') || brooklynRoutes.includes(routeId)) {
            return { condition: 'Moderate Traffic', color: '#f59e0b' };
        } else {
            return { condition: 'Light Traffic', color: '#22c55e' };
        }
    }
    
    setFallbackTrafficData(vehicleId) {
        // Provide realistic fallback traffic data
        const trafficConditions = [
            { condition: 'Light Traffic', color: '#22c55e', eta: 'On time' },
            { condition: 'Moderate Traffic', color: '#f59e0b', eta: '+3 min delay' },
            { condition: 'Heavy Traffic', color: '#ef4444', eta: '+8 min delay' }
        ];
        
        const randomTraffic = trafficConditions[Math.floor(Math.random() * trafficConditions.length)];
        
        const trafficStatus = document.getElementById(`traffic-status-${vehicleId}`);
        const etaElement = document.getElementById(`eta-${vehicleId}`);
        
        if (trafficStatus) {
            trafficStatus.innerHTML = `<span style="color: ${randomTraffic.color};">${randomTraffic.condition}</span>`;
        }
        if (etaElement) {
            etaElement.textContent = randomTraffic.eta;
        }
    }

    processTrafficData(trafficData) {
        if (!trafficData || trafficData.status !== 'success') return;

        const { vehicle_id, traffic_delay_seconds, congestion_level } = trafficData;
        
        // Show incident alert for heavy traffic
        if (congestion_level === 'heavy' && traffic_delay_seconds > 600) {
            this.showTrafficIncident({
                vehicleId: vehicle_id,
                delay: Math.round(traffic_delay_seconds / 60),
                severity: congestion_level,
                route: trafficData.route
            });
        }

    }

    showTrafficIncident(incident) {
        const mapContainer = document.getElementById('mapView');
        if (!mapContainer) return;

        // Remove existing alerts
        const existingAlert = mapContainer.querySelector('.incident-alert');
        if (existingAlert) {
            existingAlert.remove();
        }

        const alert = document.createElement('div');
        alert.className = 'incident-alert';
        alert.innerHTML = `
            <div class="flex items-start justify-between mb-2">
                <div class="flex items-center space-x-2">
                    <span class="text-lg">🚨</span>
                    <h4 class="font-bold text-sm">Traffic Alert</h4>
                </div>
                <button onclick="this.parentElement.parentElement.remove()" class="text-white hover:text-red-200 text-lg leading-none">&times;</button>
            </div>
            <div class="text-xs space-y-1">
                <div><strong>Vehicle:</strong> ${incident.vehicleId}</div>
                <div><strong>Delay:</strong> ${incident.delay} minutes</div>
                <div><strong>Severity:</strong> ${incident.severity.toUpperCase()}</div>
                <div class="text-red-100 mt-2">Heavy traffic detected on route. Consider rerouting.</div>
            </div>
        `;

        mapContainer.appendChild(alert);

        // Auto-remove after 10 seconds
        setTimeout(() => {
            if (alert.parentElement) {
                alert.remove();
            }
        }, 10000);
    }

    async refreshTrafficData() {
        
        try {
            // Force refresh traffic layers by removing and re-adding them
            const trafficFlowEnabled = document.getElementById('trafficFlow')?.checked || false;
            const trafficIncidentsEnabled = document.getElementById('trafficIncidents')?.checked || false;
            
            // Remove existing layers
            if (this.map.getLayer('traffic-flow')) {
                this.map.removeLayer('traffic-flow');
                this.map.removeSource('traffic-flow');
            }
            if (this.map.getLayer('traffic-incidents')) {
                this.map.removeLayer('traffic-incidents');
                this.map.removeSource('traffic-incidents');
            }
            
            // Re-add layers if they were enabled
            setTimeout(() => {
                if (trafficFlowEnabled) {
                    this.toggleTrafficLayer('trafficFlow', true);
                }
                if (trafficIncidentsEnabled) {
                    this.toggleTrafficLayer('trafficIncidents', true);
                }
            }, 500);
        } catch (error) {
            console.error('Error refreshing traffic data:', error);
        }

        // Trigger immediate traffic check
        this.checkTrafficConditions();
        
        this.showNotification('Traffic data refreshed', 'success');
    }

    // Utility functions for UI interactions
    showNotification(message, type = 'info') {
        // Create notification element if it doesn't exist
        let notificationContainer = document.getElementById('notificationContainer');
        if (!notificationContainer) {
            notificationContainer = document.createElement('div');
            notificationContainer.id = 'notificationContainer';
            notificationContainer.style.cssText = `
                position: fixed;
                top: 20px;
                left: 50%;
                transform: translateX(-50%);
                z-index: 10000;
                pointer-events: none;
            `;
            document.body.appendChild(notificationContainer);
        }

        const notification = document.createElement('div');
        const colors = {
            success: 'bg-green-500',
            error: 'bg-red-500',
            warning: 'bg-yellow-500',
            info: 'bg-blue-500'
        };
        
        notification.className = `${colors[type] || colors.info} text-white px-4 py-2 rounded-lg shadow-lg mb-2 text-sm font-medium`;
        notification.style.cssText = `
            animation: slideDown 0.3s ease-out;
            pointer-events: auto;
        `;
        notification.textContent = message;

        notificationContainer.appendChild(notification);

        // Auto-remove after 3 seconds
        setTimeout(() => {
            if (notification.parentElement) {
                notification.style.animation = 'slideUp 0.3s ease-in';
                setTimeout(() => notification.remove(), 300);
            }
        }, 3000);
    }

    showLoading() {
        const overlay = document.getElementById('loadingOverlay');
        if (overlay) {
            overlay.classList.remove('hidden');
        }
    }

    hideLoading() {
        const overlay = document.getElementById('loadingOverlay');
        if (overlay) {
            overlay.classList.add('hidden');
        }
    }

    // Vehicle interaction functions
    trackVehicle(vehicleId) {
        this.showNotification(`Now tracking vehicle ${vehicleId}`, 'info');
        
        // Find and highlight the vehicle marker
        const marker = this.vehicleMarkers.get(vehicleId);
        if (marker) {
            const lngLat = marker.getLngLat();
            this.map.flyTo({
                center: [lngLat.lng, lngLat.lat],
                zoom: 15,
                duration: 1000
            });
        }
    }

    toggleVehicleDetails(vehicleId) {
        
        const detailsSection = document.getElementById(`details-${vehicleId}`);
        const detailsBtn = document.getElementById(`details-btn-${vehicleId}`);
        const popup = document.getElementById(`popup-${vehicleId}`);
        const commPanel = document.getElementById(`comm-${vehicleId}`);
        
        if (!detailsSection || !detailsBtn || !popup) {
            return;
        }
        
        const isHidden = detailsSection.classList.contains('hidden');
        const isMobile = window.innerWidth <= 768;

        const COLLAPSED_W = isMobile ? '90vw' : '480px';   // ✅ wider for content
        const DETAILS_W   = isMobile ? '90vw' : '720px';   // ✅ wider for details
        const COMM_W      = isMobile ? '90vw' : '860px';
        
        if (isHidden) {
            // Show - expand horizontally
            detailsSection.classList.remove('hidden');
            popup.style.width = DETAILS_W;
            // Also update TomTom popup wrapper
            const tomtomPopup = popup.closest('.tt-popup');
            if (tomtomPopup) tomtomPopup.style.width = DETAILS_W;
            detailsBtn.textContent = 'Hide Details';
            
            // Only adjust map if popup is completely off-screen (less aggressive)
            setTimeout(() => {
                const marker = this.vehicleMarkers.get(vehicleId);
                if (marker) {
                    const coordinates = marker.getLngLat();
                    this.ensurePopupInViewGentle(vehicleId, [coordinates.lng, coordinates.lat]);
                }
            }, 100);
            
            // Load real-time TomTom data when details are expanded
            this.loadVehicleRouteData(vehicleId);
        } else {
            // Hide - contract horizontally
            detailsSection.classList.add('hidden');
            if (commPanel) commPanel.classList.add('hidden');
            popup.style.width = COLLAPSED_W;
            // Also update TomTom popup wrapper
            const tomtomPopup = popup.closest('.tt-popup');
            if (tomtomPopup) tomtomPopup.style.width = COLLAPSED_W;
            detailsBtn.textContent = 'Show Details';
        }
    }
    
    showCallPanel(vehicleId, driverName) {
        const popup = document.getElementById(`popup-${vehicleId}`);
        const detailsSection = document.getElementById(`details-${vehicleId}`);
        const commPanel = document.getElementById(`comm-${vehicleId}`);
        const callPanel = document.getElementById(`call-panel-${vehicleId}`);
        const messagePanel = document.getElementById(`message-panel-${vehicleId}`);
        
        if (!popup || !commPanel || !callPanel) return;
        
        // Ensure details are expanded first
        if (detailsSection && detailsSection.classList.contains('hidden')) {
            this.toggleVehicleDetails(vehicleId);
        }
        
        // Expand popup further for communication
        const commWidth = (window.innerWidth <= 768) ? '90vw' : '860px';
        popup.style.width = commWidth;
        // Also update TomTom popup wrapper
        const tomtomPopup = popup.closest('.tt-popup');
        if (tomtomPopup) tomtomPopup.style.width = commWidth;
        
        // Show communication panel and call interface
        commPanel.classList.remove('hidden');
        callPanel.classList.remove('hidden');
        if (messagePanel) messagePanel.classList.add('hidden');
        
        this.showNotification(`Calling ${driverName}...`, 'info');
    }
    
    showMessagePanel(vehicleId, driverName) {
        const popup = document.getElementById(`popup-${vehicleId}`);
        const detailsSection = document.getElementById(`details-${vehicleId}`);
        const commPanel = document.getElementById(`comm-${vehicleId}`);
        const callPanel = document.getElementById(`call-panel-${vehicleId}`);
        const messagePanel = document.getElementById(`message-panel-${vehicleId}`);
        
        if (!popup || !commPanel || !messagePanel) return;
        
        // Ensure details are expanded first
        if (detailsSection && detailsSection.classList.contains('hidden')) {
            this.toggleVehicleDetails(vehicleId);
        }
        
        // Expand popup further for communication
        const commWidth = (window.innerWidth <= 768) ? '90vw' : '860px';
        popup.style.width = commWidth;
        // Also update TomTom popup wrapper
        const tomtomPopup = popup.closest('.tt-popup');
        if (tomtomPopup) tomtomPopup.style.width = commWidth;
        
        // Show communication panel and message interface
        commPanel.classList.remove('hidden');
        messagePanel.classList.remove('hidden');
        if (callPanel) callPanel.classList.add('hidden');
    }
    
    closeCommPanel(vehicleId) {
        const popup = document.getElementById(`popup-${vehicleId}`);
        const commPanel = document.getElementById(`comm-${vehicleId}`);
        
        if (!popup || !commPanel) return;
        
        commPanel.classList.add('hidden');
        const detailsWidth = (window.innerWidth <= 768) ? '90vw' : '720px';
        popup.style.width = detailsWidth;
        // Also update TomTom popup wrapper
        const tomtomPopup = popup.closest('.tt-popup');
        if (tomtomPopup) tomtomPopup.style.width = detailsWidth;
    }
    
    endCall(vehicleId) {
        this.closeCommPanel(vehicleId);
        this.showNotification('Call ended', 'info');
    }
    
    sendCustomMessage(vehicleId, driverName) {
        const textarea = document.getElementById(`custom-msg-${vehicleId}`);
        const message = textarea?.value.trim();
        
        if (!message) {
            this.showNotification('Please enter a message', 'error');
            return;
        }
        
        this.showNotification(`Message sent to ${driverName}`, 'success');
        
        textarea.value = '';
        this.closeCommPanel(vehicleId);
    }

    vehicleDetails(vehicleId) {
        // This function is now replaced by toggleVehicleDetails for inline expansion
        this.toggleVehicleDetails(vehicleId);
    }
    
    showVehicleDetailsModal(vehicle) {
        const modal = document.createElement('div');
        modal.className = 'fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50';
        modal.innerHTML = `
            <div class="bg-white rounded-2xl shadow-2xl max-w-2xl w-full mx-4 max-h-[90vh] overflow-y-auto">
                <div class="p-6">
                    <div class="flex items-center justify-between mb-6">
                        <div class="flex items-center space-x-4">
                            <div class="w-16 h-16 rounded-xl flex items-center justify-center" style="background: linear-gradient(135deg, ${vehicle.statusColor}, ${this.darkenColor(vehicle.statusColor, 0.2)}); box-shadow: 0 8px 24px rgba(0,0,0,0.15);">
                                <span style="font-size: 24px; filter: drop-shadow(0 2px 4px rgba(0,0,0,0.3));">${vehicle.vehicleIcon}</span>
                            </div>
                            <div>
                                <h2 class="text-2xl font-bold text-slate-900">${vehicle.vehicleId}</h2>
                                <p class="text-slate-600 capitalize">${vehicle.vehicleType.replace('_', ' ')}</p>
                            </div>
                        </div>
                        <button onclick="this.closest('.fixed').remove()" class="text-slate-400 hover:text-slate-600 text-2xl font-bold">&times;</button>
                    </div>
                    
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
                        <div class="bg-slate-50 rounded-xl p-4">
                            <h3 class="text-lg font-semibold text-slate-900 mb-3">Status & Location</h3>
                            <div class="space-y-3">
                                <div class="flex justify-between items-center">
                                    <span class="text-slate-600">Status:</span>
                                    <span class="px-3 py-1 text-xs font-semibold rounded-full capitalize text-white" style="background: ${vehicle.statusColor};">${vehicle.status}</span>
                                </div>
                                <div class="flex justify-between items-center">
                                    <span class="text-slate-600">GPS Location:</span>
                                    <span class="text-slate-900 font-mono text-sm">${parseFloat(vehicle.lat)?.toFixed(4)}, ${parseFloat(vehicle.lng)?.toFixed(4)}</span>
                                </div>
                                <div class="flex justify-between items-center">
                                    <span class="text-slate-600">Type:</span>
                                    <span class="text-slate-900 font-semibold">${vehicle.vehicleType.replace('_', ' ')}</span>
                                </div>
                            </div>
                        </div>
                        
                        <div class="bg-slate-50 rounded-xl p-4">
                            <h3 class="text-lg font-semibold text-slate-900 mb-3">Performance Metrics</h3>
                            <div class="space-y-4">
                                <div>
                                    <div class="flex justify-between items-center mb-2">
                                        <span class="text-slate-600">Fuel Level</span>
                                        <span class="text-slate-900 font-semibold">${vehicle.fuelLevel}%</span>
                                    </div>
                                    <div class="w-full h-3 bg-slate-200 rounded-full overflow-hidden">
                                        <div class="h-full rounded-full transition-all" style="width: ${vehicle.fuelLevel}%; background: ${vehicle.fuelLevel > 50 ? '#22c55e' : vehicle.fuelLevel > 25 ? '#f59e0b' : '#ef4444'};"></div>
                                    </div>
                                </div>
                                
                                <div>
                                    <div class="flex justify-between items-center mb-2">
                                        <span class="text-slate-600">Maintenance Score</span>
                                        <span class="text-slate-900 font-semibold">${vehicle.maintenanceScore}/100</span>
                                    </div>
                                    <div class="w-full h-3 bg-slate-200 rounded-full overflow-hidden">
                                        <div class="h-full rounded-full transition-all" style="width: ${vehicle.maintenanceScore}%; background: ${vehicle.maintenanceScore > 70 ? '#22c55e' : vehicle.maintenanceScore > 40 ? '#f59e0b' : '#ef4444'};"></div>
                                    </div>
                                </div>
                                
                                <div class="flex justify-between items-center">
                                    <span class="text-slate-600">Capacity:</span>
                                    <span class="text-slate-900 font-semibold">${vehicle.capacity} kg</span>
                                </div>
                            </div>
                        </div>
                    </div>
                    
                    <div class="bg-blue-50 rounded-xl p-4 mb-6">
                        <h3 class="text-lg font-semibold text-slate-900 mb-3">Quick Actions</h3>
                        <div class="flex space-x-3">
                            <button onclick="fleetCommand.trackVehicle('${vehicle.vehicleId}')" class="flex-1 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors font-medium">
                                Track Vehicle
                            </button>
                            <button onclick="fleetCommand.centerMapOnVehicle('${vehicle.vehicleId}'); this.closest('.fixed').remove();" class="flex-1 px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 transition-colors font-medium">
                                Show on Map
                            </button>
                        </div>
                    </div>
                    
                    <div class="text-center">
                        <button onclick="this.closest('.fixed').remove()" class="px-6 py-2 bg-slate-200 text-slate-700 rounded-lg hover:bg-slate-300 transition-colors font-medium">
                            Close
                        </button>
                    </div>
                </div>
            </div>
        `;
        
        document.body.appendChild(modal);
        modal.addEventListener('click', (e) => {
            if (e.target === modal) {
                modal.remove();
            }
        });
    }
    
    async getDriverForVehicle(vehicleId) {
        try {
            const response = await fetch(`${this.apiBaseUrl}/employees`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `
                        match
                        $assignment isa assignment (assigned-vehicle: $vehicle, assigned-employee: $employee);
                        $vehicle has vehicle-id "${vehicleId}";
                        $employee has employee-name $name,
                                   has performance-rating $rating,
                                   has certifications $certs,
                                   has employee-role $role,
                                   has shift-schedule $schedule;
                        select $employee, $name, $schedule, $rating, $certs, $role;
                    `
                })
            });

            if (!response.ok) {
                return null;
            }

            const data = await response.json();
            
            if (data.ok && data.ok.answers && data.ok.answers.length > 0) {
                const employee = data.ok.answers[0].data;
                return {
                    name: employee.name?.value || 'Unknown Driver',
                    status: employee.schedule?.value || 'unknown',
                    rating: employee.rating?.value || 0,
                    certifications: employee.certs?.value || 'N/A',
                    role: employee.role?.value || 'driver'
                };
            }
            
            return null;
        } catch (error) {
            this.showNotification(`Error fetching driver for vehicle ${vehicleId}`, 'error');
            return null;
        }
    }

    async loadVehicleRouteData(vehicleId) {
        try {
            // Get vehicle current location
            const vehicle = this.data.vehicles.find(v => v.data?.id?.value === vehicleId);
            if (!vehicle) return;

            const currentLat = parseFloat(vehicle.data.lat?.value);
            const currentLng = parseFloat(vehicle.data.lng?.value);

            // Get assigned delivery for this vehicle to determine destination
            const deliveryResponse = await fetch(`${this.apiBaseUrl}/deliveries`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `
                        match 
                        $assignment isa assignment (assigned-vehicle: $vehicle, assigned-delivery: $delivery);
                        $vehicle has vehicle-id "${vehicleId}";
                        $delivery has route-id $routeId,
                                 has pickup-address $pickup,
                                 has delivery-address $destination,
                                 has delivery-time $deliveryTime,
                                 has status $deliveryStatus;
                        select $routeId, $pickup, $destination, $deliveryTime, $deliveryStatus;
                    `
                })
            });

            if (deliveryResponse.ok) {
                const deliveryData = await deliveryResponse.json();
                if (deliveryData.deliveries && deliveryData.deliveries.length > 0) {
                    const delivery = deliveryData.deliveries[0].data;
                    const destLat = parseFloat(delivery.dest_lat?.value);
                    const destLng = parseFloat(delivery.dest_lng?.value);

                    if (destLat && destLng) {
                        // Fetch real-time traffic data from TomTom API
                        await this.fetchTomTomTrafficData(vehicleId, currentLat, currentLng, destLat, destLng);
                        
                        // Fetch ETA data
                        await this.fetchTomTomETA(vehicleId, currentLat, currentLng, destLat, destLng);
                    }
                }
            }

            // Update route status
            document.getElementById(`route-status-${vehicleId}`).textContent = 'Active Route';
            
        } catch (error) {
            this.showNotification(`Error loading route data for vehicle ${vehicleId}`, 'error');
            document.getElementById(`route-status-${vehicleId}`).textContent = 'Error loading';
            document.getElementById(`traffic-status-${vehicleId}`).textContent = 'Unavailable';
            document.getElementById(`eta-${vehicleId}`).textContent = 'Unavailable';
        }
    }

    async fetchTomTomTrafficData(vehicleId, fromLat, fromLng, toLat, toLng) {
        try {
            const response = await fetch('/api/tomtom/traffic', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    vehicle_id: vehicleId,
                    from_lat: fromLat,
                    from_lng: fromLng,
                    to_lat: toLat,
                    to_lng: toLng
                })
            });

            if (response.ok) {
                const trafficData = await response.json();
                const congestionLevel = trafficData.congestion_level || 'unknown';
                const delayMinutes = Math.round((trafficData.traffic_delay_seconds || 0) / 60);
                
                let trafficStatus = congestionLevel.charAt(0).toUpperCase() + congestionLevel.slice(1);
                if (delayMinutes > 0) {
                    trafficStatus += ` (+${delayMinutes}min delay)`;
                }
                
                document.getElementById(`traffic-status-${vehicleId}`).textContent = trafficStatus;
            } else {
                document.getElementById(`traffic-status-${vehicleId}`).textContent = 'Unknown';
            }
        } catch (error) {
            this.showNotification(`Error fetching traffic data for vehicle ${vehicleId}`, 'error');
            document.getElementById(`traffic-status-${vehicleId}`).textContent = 'Error';
        }
    }

    async fetchTomTomETA(vehicleId, fromLat, fromLng, toLat, toLng) {
        try {
            const response = await fetch('/api/tomtom/eta', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({
                    vehicle_id: vehicleId,
                    delivery_id: 'current', // Could be more specific
                    current_lat: fromLat,
                    current_lng: fromLng,
                    dest_lat: toLat,
                    dest_lng: toLng
                })
            });

            if (response.ok) {
                const etaData = await response.json();
                const remainingMinutes = Math.round((etaData.remaining_time_seconds || 0) / 60);
                const etaTime = new Date(etaData.estimated_arrival);
                
                const etaString = `${remainingMinutes}min (${etaTime.toLocaleTimeString()})`;
                document.getElementById(`eta-${vehicleId}`).textContent = etaString;
            } else {
                document.getElementById(`eta-${vehicleId}`).textContent = 'Unknown';
            }
        } catch (error) {
            this.showNotification(`Error fetching ETA for vehicle ${vehicleId}`, 'error');
            document.getElementById(`eta-${vehicleId}`).textContent = 'Error';
        }
    }

    centerMapOnVehicle(vehicleId) {
        const vehicle = this.data.vehicles.find(v => 
            v.data?.id?.value === vehicleId
        );
        
        if (vehicle && this.map) {
            const lat = parseFloat(vehicle.data.lat?.value);
            const lng = parseFloat(vehicle.data.lng?.value);
            
            this.map.setCenter([lng, lat]);
        }
    }
    
    closeCommPanel(vehicleId) {
        const popup = document.getElementById(`popup-${vehicleId}`);
        const commPanel = document.getElementById(`comm-${vehicleId}`);
        
        if (!popup || !commPanel) return;
        
        commPanel.classList.add('hidden');
        const detailsWidth = (window.innerWidth <= 768) ? '90vw' : '720px';
        popup.style.width = detailsWidth;
        // Also update TomTom popup wrapper
        const tomtomPopup = popup.closest('.tt-popup');
        if (tomtomPopup) tomtomPopup.style.width = detailsWidth;
    }

    trackVehicle(vehicleId) {
        this.showNotification(`Started tracking vehicle ${vehicleId}`, 'success');
        
        // In a real implementation, this would start real-time tracking
        // For now, just center the map on the vehicle
        this.centerMapOnVehicle(vehicleId);
    }
    
    callDriver(vehicleId, driverName) {
        
        // In a real implementation, this would initiate a VoIP call or phone call
        const modal = document.createElement('div');
        modal.className = 'fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50';
        modal.innerHTML = `
            <div class="bg-white rounded-2xl shadow-2xl max-w-md w-full mx-4 p-6">
                <div class="text-center">
                    <div class="w-16 h-16 bg-green-100 rounded-full flex items-center justify-center mx-auto mb-4">
                        <span class="text-2xl">📞</span>
                    </div>
                    <h3 class="text-lg font-semibold text-slate-900 mb-2">Calling ${driverName}</h3>
                    <p class="text-slate-600 mb-4">Vehicle: ${vehicleId}</p>
                    <div class="flex space-x-3">
                        <button onclick="this.closest('.fixed').remove()" class="flex-1 px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors">
                            End Call
                        </button>
                        <button onclick="this.closest('.fixed').remove()" class="flex-1 px-4 py-2 bg-slate-200 text-slate-700 rounded-lg hover:bg-slate-300 transition-colors">
                            Cancel
                        </button>
                    </div>
                </div>
            </div>
        `;
        document.body.appendChild(modal);
        
        this.showNotification(`Calling ${driverName}...`, 'info');
    }
    
    messageDriver(vehicleId, driverName) {
        
        const modal = document.createElement('div');
        modal.className = 'fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50';
        modal.innerHTML = `
            <div class="bg-white rounded-2xl shadow-2xl max-w-md w-full mx-4 p-6">
                <div class="flex items-center justify-between mb-4">
                    <h3 class="text-lg font-semibold text-slate-900">Message ${driverName}</h3>
                    <button onclick="this.closest('.fixed').remove()" class="text-slate-400 hover:text-slate-600 text-xl">&times;</button>
                </div>
                <p class="text-slate-600 mb-4">Vehicle: ${vehicleId}</p>
                
                <div class="mb-4">
                    <label class="block text-sm font-medium text-slate-700 mb-2">Quick Messages:</label>
                    <div class="grid grid-cols-1 gap-2">
                        <button onclick="fleetCommand.sendQuickMessage('${vehicleId}', '${driverName}', 'Please update your status')" class="text-left px-3 py-2 bg-slate-100 hover:bg-slate-200 rounded-lg text-sm transition-colors">
                            📍 Please update your status
                        </button>
                        <button onclick="fleetCommand.sendQuickMessage('${vehicleId}', '${driverName}', 'Return to depot when current delivery is complete')" class="text-left px-3 py-2 bg-slate-100 hover:bg-slate-200 rounded-lg text-sm transition-colors">
                            🏠 Return to depot when current delivery is complete
                        </button>
                        <button onclick="fleetCommand.sendQuickMessage('${vehicleId}', '${driverName}', 'Take your scheduled break')" class="text-left px-3 py-2 bg-slate-100 hover:bg-slate-200 rounded-lg text-sm transition-colors">
                            ☕ Take your scheduled break
                        </button>
                    </div>
                </div>
                
                <div class="mb-4">
                    <label class="block text-sm font-medium text-slate-700 mb-2">Custom Message:</label>
                    <textarea id="custom-message-${vehicleId}" class="w-full px-3 py-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500" rows="3" placeholder="Type your message..."></textarea>
                </div>
                
                <div class="flex space-x-3">
                    <button onclick="fleetCommand.sendCustomMessage('${vehicleId}', '${driverName}')" class="flex-1 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors">
                        Send Message
                    </button>
                    <button onclick="this.closest('.fixed').remove()" class="px-4 py-2 bg-slate-200 text-slate-700 rounded-lg hover:bg-slate-300 transition-colors">
                        Cancel
                    </button>
                </div>
            </div>
        `;
        document.body.appendChild(modal);
    }
    
    requestStatus(vehicleId, driverName) {
        
        // In a real implementation, this would send a status request to the driver's device
        this.showNotification(`Status request sent to ${driverName}`, 'success');
        
        // Simulate receiving a status update after a delay
        setTimeout(() => {
            const statusUpdates = [
                'Currently en route to delivery location',
                'Completed delivery, heading to next stop',
                'Taking scheduled break',
                'Refueling at station',
                'Waiting for loading at warehouse'
            ];
            const randomStatus = statusUpdates[Math.floor(Math.random() * statusUpdates.length)];
            this.showNotification(`${driverName}: ${randomStatus}`, 'info');
        }, 2000);
    }
    
    sendQuickMessage(vehicleId, driverName, message) {
        this.showNotification(`Message sent to ${driverName}: "${message}"`, 'success');
        
        // Close the message modal
        const modal = document.querySelector('.fixed');
        if (modal) modal.remove();
    }
    
    sendCustomMessage(vehicleId, driverName) {
        const textarea = document.getElementById(`custom-message-${vehicleId}`);
        const message = textarea?.value.trim();
        
        if (!message) {
            this.showNotification('Please enter a message', 'error');
            return;
        }
        
        this.showNotification(`Custom message sent to ${driverName}`, 'success');
        
        // Close the message modal
        const modal = document.querySelector('.fixed');
        if (modal) modal.remove();
    }

    viewDelivery(deliveryId) {
        this.showNotification(`Loading delivery ${deliveryId}`, 'info');
    }
    
    updateUI() {
        this.updateMetrics();
        this.updateCurrentView();
        this.updateLiveActivity();
    }
    
    updateMetrics() {
        // Active vehicles
        const activeVehicles = this.data.vehicles.filter(v => 
            v.data?.vehicle?.status === 'operational' || v.data?.vehicle?.status === 'busy'
        ).length;
        const activeVehiclesElement = document.getElementById('activeVehicles');
        if (activeVehiclesElement) {
            activeVehiclesElement.textContent = activeVehicles;
        }
        
        // Pending deliveries
        const pendingDeliveries = this.data.deliveries.filter(d => 
            d.data?.delivery?.status === 'pending'
        ).length;
        const pendingDeliveriesElement = document.getElementById('pendingDeliveries');
        if (pendingDeliveriesElement) {
            pendingDeliveriesElement.textContent = pendingDeliveries;
        }
    }
    
    updateCurrentView() {
        switch(this.currentView) {
            case 'dispatch':
                this.updateDispatchView();
                break;
            case 'fleet':
                this.updateFleetView();
                break;
            case 'drivers':
                this.updateDriversView();
                break;
            case 'analytics':
                this.updateAnalyticsView();
                break;
            case 'maintenance':
                this.updateMaintenanceView();
                break;
            case 'system':
                this.updateSystemView();
                break;
        }
    }
    
    updateDispatchView() {
        this.updateAvailableDrivers();
        this.updatePendingQueue();
    }
    
    updateAvailableDrivers() {
        const container = document.getElementById('availableDrivers');
        if (!container) return;
        
        const availableDrivers = this.data.drivers.filter(d => 
            d.data?.emp?.status === 'available'
        ).slice(0, 8);
        
        const driversHtml = availableDrivers.map(driver => {
            const driverData = driver.data?.emp || {};
            const name = driverData.name || 'Unknown Driver';
            const rating = driverData.rating || 4.5;
            const hours = driverData['daily-hours'] || 0;
            const driverId = driverData.id || driver.data?.emp?.id || 'UNKNOWN';
            
            const statusColor = hours > 8 ? 'danger' : hours > 6 ? 'warning' : 'success';
            const statusClass = hours > 8 ? 'danger' : hours > 6 ? 'warning' : 'online';
            
            return `
                <div class="bg-slate-50 rounded-lg p-3 cursor-pointer fade-in" 
                     data-driver-id="${driverId}" 
                     draggable="false">
                    <div class="flex items-center justify-between mb-2">
                        <div class="flex items-center gap-2">
                            <div style="width: 20px; height: 20px; display: flex; align-items: center; justify-content: center; border-radius: 6px; background: #f1f5f9;">
                                <svg class="text-gray-600 w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"/>
                                </svg>
                            </div>
                            <div>
                                <h4 class="text-sm font-medium">${name}</h4>
                                <div class="flex items-center gap-1 mt-0.5">
                                    <div class="w-2 h-2 bg-${statusColor === 'success' ? 'green' : statusColor === 'warning' ? 'yellow' : 'red'}-500 rounded-full"></div>
                                    <span class="text-xs text-slate-600">${hours.toFixed(1)}h / 11h worked</span>
                                </div>
                            </div>
                        </div>
                        <div class="text-right">
                            <div class="text-sm font-medium">${rating.toFixed(1)}</div>
                            <div class="text-xs text-slate-600">Rating</div>
                        </div>
                    </div>
                    <div class="w-full bg-gray-200 rounded-full h-1.5">
                        <div class="bg-${statusColor === 'success' ? 'green' : statusColor === 'warning' ? 'yellow' : 'red'}-500 h-1.5 rounded-full transition-all" style="width: ${Math.min((hours/11)*100, 100)}%"></div>
                    </div>
                </div>
            `;
        }).join('');
        
        container.innerHTML = driversHtml || '<div class="text-center py-6"><p class="text-body-md">No available drivers</p></div>';
    }
    
    updatePendingQueue() {
        const container = document.getElementById('pendingQueue');
        if (!container) return;
        
        const pendingDeliveries = this.data.deliveries.filter(d => 
            d.data?.delivery?.status === 'pending'
        ).slice(0, 6);
        
        const deliveriesHtml = pendingDeliveries.map(delivery => {
            const deliveryData = delivery.data?.delivery || {};
            
            // Skip deliveries without required database fields
            if (!deliveryData['delivery-id'] || !deliveryData['customer-name'] || 
                !deliveryData['customer-priority'] || !deliveryData['delivery-address'] ||
                !deliveryData['priority-color'] || !deliveryData['priority-icon'] || 
                !deliveryData['priority-label']) {
                return '';
            }
            
            const deliveryId = deliveryData['delivery-id'];
            const customer = deliveryData['customer-name'];
            const priority = deliveryData['customer-priority'];
            const address = deliveryData['delivery-address'];
            const priorityColor = deliveryData['priority-color'];
            const priorityIcon = deliveryData['priority-icon'];
            const priorityLabel = deliveryData['priority-label'];
            
            return `
                <div class="vehicle-card cursor-move fade-in" 
                     data-delivery-id="${deliveryId}" 
                     draggable="true">
                    <div class="flex items-center justify-between mb-3">
                        <div class="flex items-center gap-3">
                            <div class="metric-icon bg-${priorityColor}-100">
                                <span class="text-lg">${priorityIcon}</span>
                            </div>
                            <div>
                                <h4 class="text-heading-sm">${customer}</h4>
                                <p class="text-body-sm mt-1">${address}</p>
                            </div>
                        </div>
                        <div class="vehicle-status ${priorityColor === 'red' ? 'maintenance' : priorityColor === 'amber' ? 'maintenance' : 'operational'}">
                            ${priorityLabel}
                        </div>
                    </div>
                    <div class="flex items-center justify-between">
                        <span class="text-body-sm font-mono text-gray-500">${deliveryId}</span>
                        <button class="btn-secondary text-xs px-2 py-1" onclick="fleetCommand.viewDelivery('${deliveryId}')">
                            View Details
                        </button>
                    </div>
                </div>
            `;
        }).join('');
        
        container.innerHTML = deliveriesHtml || '<div class="text-center py-6"><p class="text-body-md">No pending deliveries</p></div>';
    }
    
    updateLiveActivity() {
        const container = document.getElementById('liveActivity');
        if (!container) return;
        
        const currentTime = new Date().toLocaleTimeString('en-US', { 
            hour12: false, 
            hour: '2-digit', 
            minute: '2-digit',
            second: '2-digit'
        });
        
        const activities = [
            { 
                text: `${this.data.vehicles.length} vehicles operational`, 
                time: currentTime, 
                type: 'operational',
                icon: '🚛'
            },
            { 
                text: `${this.data.operations.length} active operations`, 
                time: currentTime, 
                type: 'success',
                icon: '⚡'
            },
            { 
                text: `${this.data.drivers.filter(d => d.data?.emp?.status === 'available').length} drivers available`, 
                time: currentTime, 
                type: 'info',
                icon: '👥'
            },
            { 
                text: `System performance optimal`, 
                time: currentTime, 
                type: 'success',
                icon: '✅'
            }
        ];
        
        const activityHtml = activities.map(activity => `
            <div class="flex items-center gap-4 py-3 border-b border-gray-100 last:border-b-0">
                <div class="metric-icon bg-gray-100">
                    <span class="text-lg">${activity.icon}</span>
                </div>
                <div class="flex-1">
                    <p class="text-body-lg font-medium">${activity.text}</p>
                    <p class="text-body-sm mt-1">Updated ${activity.time}</p>
                </div>
                <div class="status-dot ${activity.type === 'success' ? 'online' : activity.type === 'operational' ? 'online' : 'offline'}"></div>
            </div>
        `).join('');
        
        container.innerHTML = activityHtml;
    }
    
    // Smart Assignment Functions (Using TypeDB Functions)
    async runSmartAssignment() {
        this.showLoading();
        try {
            // Use TypeDB premium_delivery_assignments function
            const response = await fetch(`${this.apiBaseUrl}/deliveries`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $result = premium_delivery_assignments($delivery); $delivery isa delivery, has customer-priority "premium"; select $result;'
                })
            });
            
            const result = await response.json();
            
            this.showNotification('Premium deliveries auto-assigned to top drivers', 'success');
            await this.loadAllData();
            this.updateUI();
        } catch (error) {
            console.error('Smart assignment failed:', error);
            this.showNotification('Smart assignment failed', 'error');
        } finally {
            this.hideLoading();
        }
    }
    
    async handleUrgentDeliveries() {
        this.showLoading();
        try {
            // Use TypeDB urgent_delivery_assignments function
            const response = await fetch(`${this.apiBaseUrl}/deliveries`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $result = urgent_delivery_assignments($delivery); $delivery isa delivery, has customer-priority "urgent"; select $result;'
                })
            });
            
            const result = await response.json();
            
            this.showNotification('Emergency deliveries dispatched immediately', 'success');
            await this.loadAllData();
            this.updateUI();
        } catch (error) {
            console.error('Emergency dispatch failed:', error);
            this.showNotification('Emergency dispatch failed', 'error');
        } finally {
            this.hideLoading();
        }
    }
    
    async optimizeRoutes() {
        this.showLoading();
        try {
            // Trigger route rebalancing job
            const response = await fetch(`${this.apiBaseUrl}/jobs`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'enqueue'
                },
                body: JSON.stringify({
                    job_type: 'route_rebalancing',
                    manual_trigger: true,
                    timestamp: new Date().toISOString()
                })
            });
            
            if (response.ok) {
                this.showNotification('Route optimization started', 'success');
            }
        } catch (error) {
            console.error('Route optimization failed:', error);
            this.showNotification('Route optimization failed', 'error');
        } finally {
            this.hideLoading();
        }
    }
    
    async assignDeliveryToDriver(deliveryId, driverId) {
        try {
            console.log(`🎯 Assigning delivery ${deliveryId} to driver ${driverId}`);
            
            // Create assignment in TypeDB
            const response = await fetch(`${this.apiBaseUrl}/operations`, {
                method: 'POST',
                headers: { 
                    'Content-Type': 'application/json',
                    'x-service-method': 'write'
                },
                body: JSON.stringify({
                    query: `match $d isa delivery, has delivery-id "${deliveryId}"; $emp isa employee, has id "${driverId}"; insert (assigned-delivery: $d, assigned-employee: $emp) isa assignment, has timestamp ${new Date().toISOString()}, has status "active";`
                })
            });
            
            if (response.ok) {
                this.showNotification(`Delivery assigned to driver successfully`, 'success');
                await this.loadAllData();
                this.updateUI();
            }
        } catch (error) {
            console.error('Assignment failed:', error);
            this.showNotification('Assignment failed', 'error');
        }
    }
    
    startRealTimeUpdates() {
        // Update data every 30 seconds
        this.updateIntervals.set('data', setInterval(() => {
            this.loadAllData().then(() => this.updateUI());
        }, 30000));
        
        // Update live activity every 5 seconds
        this.updateIntervals.set('activity', setInterval(() => {
            this.updateLiveActivity();
        }, 5000));
        
        // Map marker updates disabled - should only update when real vehicle positions change
        // TODO: Enable when real GPS tracking is integrated
        console.log('🗺️ Map marker auto-updates disabled - awaiting real vehicle GPS data');
    }
    
    showNotification(message, type = 'info') {
        const notification = document.createElement('div');
        const colors = {
            success: 'bg-green-500',
            error: 'bg-red-500',
            warning: 'bg-yellow-500',
            info: 'bg-blue-500'
        };
        
        notification.className = `${colors[type]} text-white px-6 py-3 rounded-lg shadow-lg animate-slide-up`;
        notification.textContent = message;
        
        document.getElementById('notifications').appendChild(notification);
        
        setTimeout(() => {
            notification.remove();
        }, 4000);
    }
    
    showLoading() {
        const overlay = document.getElementById('loadingOverlay');
        if (overlay) {
            overlay.classList.remove('hidden');
        }
        
        // Add loading state to panels
        document.querySelectorAll('.panel-section').forEach(panel => {
            panel.classList.add('loading-state');
        });
    }
    
    hideLoading() {
        const overlay = document.getElementById('loadingOverlay');
        if (overlay) {
            overlay.classList.add('hidden');
        }
        
        // Remove loading state from panels
        document.querySelectorAll('.panel-section').forEach(panel => {
            panel.classList.remove('loading-state');
        });
    }


async assignDeliveryToDriver(deliveryId, driverId) {
    try {
        console.log(`🎯 Assigning delivery ${deliveryId} to driver ${driverId}`);
        
        // Create assignment in TypeDB
        const response = await fetch(`${this.apiBaseUrl}/operations`, {
            method: 'POST',
            headers: { 
                'Content-Type': 'application/json',
                'x-service-method': 'write'
            },
            body: JSON.stringify({
                query: `match $d isa delivery, has delivery-id "${deliveryId}"; $emp isa employee, has id "${driverId}"; insert (assigned-delivery: $d, assigned-employee: $emp) isa assignment, has timestamp ${new Date().toISOString()}, has status "active";`
            })
        });
        
        if (response.ok) {
            this.showNotification(`Delivery assigned to driver successfully`, 'success');
            await this.loadAllData();
            this.updateUI();
        }
    } catch (error) {
        console.error('Assignment failed:', error);
        this.showNotification('Assignment failed', 'error');
    }
}

startRealTimeUpdates() {
    // Update data every 30 seconds
    this.updateIntervals.set('data', setInterval(() => {
        this.loadAllData().then(() => this.updateUI());
    }, 30000));
    
    // Update live activity every 5 seconds
    this.updateIntervals.set('activity', setInterval(() => {
        this.updateLiveActivity();
    }, 5000));
    
    // Map marker updates disabled - should only update when real vehicle positions change
    // TODO: Enable when real GPS tracking is integrated
    console.log('🗺️ Map marker auto-updates disabled - awaiting real vehicle GPS data');
}

showNotification(message, type = 'info') {
    const notification = document.createElement('div');
    const colors = {
        success: 'bg-green-500',
        error: 'bg-red-500',
        warning: 'bg-yellow-500',
        info: 'bg-blue-500'
    };
        console.log('Updating drivers view...');
    }
    
    updateAnalyticsView() {
        console.log('Updating analytics view...');
    }
    
    updateMaintenanceView() {
        console.log('Updating maintenance view...');
    }
    
    updateSystemView() {
        console.log('Updating system view...');
    }
    
    // Navigation methods
    showDashboard() {
        console.log('Showing dashboard...');
        // Hide any open panels
        this.hideDriverAssignmentPanel();
    }
    
    showDriverAssignment() {
        console.log('Showing driver assignment panel...');
        this.showDriverAssignmentPanel();
    }
    
    showVehicleManagement() {
        console.log('Showing vehicle management...');
        this.hideDriverAssignmentPanel();
    }
    
    showRouteOptimization() {
        console.log('Showing route optimization...');
        this.hideDriverAssignmentPanel();
    }
    
    // Driver Assignment Panel
    showDriverAssignmentPanel() {
        // Create or show the driver assignment panel
        let panel = document.getElementById('driver-assignment-panel');
        if (!panel) {
            panel = this.createDriverAssignmentPanel();
            document.body.appendChild(panel);
        }
        panel.classList.remove('hidden');
    }
    
    hideDriverAssignmentPanel() {
        const panel = document.getElementById('driver-assignment-panel');
        if (panel) {
            panel.classList.add('hidden');
        }
    }
    
    createDriverAssignmentPanel() {
        const panel = document.createElement('div');
        panel.id = 'driver-assignment-panel';
        panel.className = 'fixed top-4 bg-white rounded-lg shadow-xl border border-gray-200 z-50 w-80 max-h-[calc(100vh-2rem)] overflow-y-auto';
        
        panel.innerHTML = `
            <div class="p-4 border-b border-gray-100">
                <div class="flex items-center justify-between">
                    <h2 class="text-lg font-semibold text-slate-900">Driver Assignment</h2>
                    <button onclick="fleetCommand.hideDriverAssignmentPanel()" class="text-slate-400 hover:text-slate-600 p-2">
                        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>
                        </svg>
                    </button>
                </div>
            </div>
            
            <div class="p-4">
                <form id="assignment-form" class="space-y-4">
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Route ID</label>
                        <input type="text" id="route-id" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" placeholder="Enter route ID">
                    </div>
                    
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Vehicle Assignment</label>
                        <select id="vehicle-assignment" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                            <option value="">Select a vehicle...</option>
                        </select>
                    </div>
                    
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Pickup Location</label>
                        <div class="relative">
                            <input type="text" id="pickup-address" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" placeholder="Enter pickup address">
                            <div id="address-suggestions" class="absolute z-10 w-full bg-white border border-gray-300 rounded-lg shadow-lg mt-1 max-h-48 overflow-y-auto hidden"></div>
                            <input type="hidden" id="pickup-lat">
                            <input type="hidden" id="pickup-lng">
                        </div>
                    </div>
                    
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Delivery Destination</label>
                        <div class="relative">
                            <input type="text" id="delivery-address" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" placeholder="Enter delivery address">
                            <div id="delivery-suggestions" class="absolute z-10 w-full bg-white border border-gray-300 rounded-lg shadow-lg mt-1 max-h-48 overflow-y-auto hidden"></div>
                            <input type="hidden" id="delivery-lat">
                            <input type="hidden" id="delivery-lng">
                        </div>
                    </div>
                    
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Delivery Priority</label>
                        <select id="delivery-priority" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                            <option value="standard">Standard</option>
                            <option value="high">High</option>
                            <option value="urgent">Urgent</option>
                            <option value="critical">Critical</option>
                        </select>
                    </div>
                    
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Required Certifications</label>
                        <div class="grid grid-cols-2 gap-2">
                            <label class="flex items-center py-1">
                                <input type="checkbox" value="CDL-A" class="certification-checkbox mr-2">
                                <span class="text-sm">CDL-A</span>
                            </label>
                            <label class="flex items-center py-1">
                                <input type="checkbox" value="CDL-B" class="certification-checkbox mr-2">
                                <span class="text-sm">CDL-B</span>
                            </label>
                            <label class="flex items-center py-1">
                                <input type="checkbox" value="Hazmat" class="certification-checkbox mr-2">
                                <span class="text-sm">Hazmat</span>
                            </label>
                            <label class="flex items-center py-1">
                                <input type="checkbox" value="Forklift" class="certification-checkbox mr-2">
                                <span class="text-sm">Forklift</span>
                            </label>
                        </div>
                    </div>
                    
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Schedule Assignment</label>
                        <input type="datetime-local" id="assignment-schedule" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                    </div>
                    
                    <div id="driver-assignment-container" class="hidden">
                        <label class="block text-sm font-medium text-slate-700 mb-2">Driver Assignment</label>
                        <select id="driver-assignment" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                            <option value="">Select a driver...</option>
                        </select>
                    </div>
                    
                    <div class="flex gap-3 pt-4">
                        <button type="submit" id="schedule-assignment-btn" class="flex-1 bg-gray-400 text-white px-4 py-2 text-sm rounded-lg cursor-not-allowed transition-colors" disabled>
                            Schedule Assignment
                        </button>
                        <button type="button" onclick="fleetCommand.hideDriverAssignmentPanel()" class="px-4 py-2 text-sm border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors">
                            Cancel
                        </button>
                    </div>
                </form>
            </div>
            
            <div class="border-t border-gray-100 p-4">
                <h3 class="text-sm font-medium text-slate-700 mb-3">Recent Assignments</h3>
                <div id="recent-assignments" class="space-y-2 max-h-32 overflow-y-auto">
                    <div class="text-sm text-slate-500">No recent assignments</div>
                </div>
            </div>
        `;
        
        // Position panel next to navigation menu
        const navPanel = document.querySelector('nav');
        if (navPanel) {
            const navRect = navPanel.getBoundingClientRect();
            panel.style.left = `${navRect.right + 16}px`;
        } else {
            panel.style.left = '288px'; // fallback
        }
        
        // Add form submit handler
        const form = panel.querySelector('#assignment-form');
        form.addEventListener('submit', (e) => {
            e.preventDefault();
            this.submitDriverAssignment();
        });
        
        // Setup address autocomplete
        this.setupAddressAutocomplete();
        
        // Setup datetime constraints
        this.setupDateTimeConstraints();
        
        // Load available vehicles and setup date-dependent driver loading
        setTimeout(() => {
            this.loadAvailableVehicles();
            this.setupDateDependentDriverLoading();
            this.setupFormValidation();
        }, 500);
        
        return panel;
    }
    
    async submitDriverAssignment() {
        const routeId = document.getElementById('route-id').value;
        const vehicleId = document.getElementById('vehicle-assignment').value;
        const driverId = document.getElementById('driver-assignment').value;
        const pickupLat = parseFloat(document.getElementById('pickup-lat').value);
        const pickupLng = parseFloat(document.getElementById('pickup-lng').value);
        const deliveryPriority = document.getElementById('delivery-priority').value;
        const scheduleTime = document.getElementById('assignment-schedule').value;
        
        // Get selected certifications
        const certificationCheckboxes = document.querySelectorAll('.certification-checkbox:checked');
        const requiredCertifications = Array.from(certificationCheckboxes).map(cb => cb.value);
        
        if (!routeId || !vehicleId || !driverId || isNaN(pickupLat) || isNaN(pickupLng)) {
            this.showNotification('Please fill in all required fields including route ID, vehicle, driver, and pickup address', 'error');
            return;
        }
        
        const deliveryLat = parseFloat(document.getElementById('delivery-lat').value);
        const deliveryLng = parseFloat(document.getElementById('delivery-lng').value);
        const deliveryAddress = document.getElementById('delivery-address').value;
        
        const assignmentData = {
            route_id: routeId,
            vehicle_id: vehicleId,
            driver_id: driverId,
            pickup_location: [pickupLat, pickupLng],
            pickup_address: document.getElementById('pickup-address').value,
            delivery_location: [deliveryLat, deliveryLng],
            delivery_address: deliveryAddress,
            delivery_priority: deliveryPriority,
            required_certifications: requiredCertifications,
            scheduled_time: scheduleTime || null
        };
        
        try {
            const response = await fetch(`${this.apiBaseUrl}/api/assignments`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify(assignmentData)
            });
            
            if (response.ok) {
                const result = await response.json();
                this.showNotification('Assignment scheduled successfully', 'success');
                this.clearAssignmentForm();
                this.loadRecentAssignments();
            } else {
                throw new Error('Failed to schedule assignment');
            }
        } catch (error) {
            console.error('Error scheduling assignment:', error);
            this.showNotification('Failed to schedule assignment', 'error');
        }
    }
    
    clearAssignmentForm() {
        document.getElementById('route-id').value = '';
        document.getElementById('vehicle-assignment').value = '';
        document.getElementById('driver-assignment').value = '';
        document.getElementById('pickup-address').value = '';
        document.getElementById('pickup-lat').value = '';
        document.getElementById('pickup-lng').value = '';
        document.getElementById('delivery-address').value = '';
        document.getElementById('delivery-lat').value = '';
        document.getElementById('delivery-lng').value = '';
        document.getElementById('delivery-priority').value = 'standard';
        document.getElementById('assignment-schedule').value = '';
        document.querySelectorAll('.certification-checkbox').forEach(cb => cb.checked = false);
        
        // Hide driver dropdown when form is cleared
        const driverContainer = document.getElementById('driver-assignment-container');
        if (driverContainer) {
            driverContainer.classList.add('hidden');
        }
    }
    
    async loadAvailableVehicles() {
        try {
            const vehicleSelect = document.getElementById('vehicle-assignment');
            if (!vehicleSelect) {
                console.error('Vehicle select element not found');
                return;
            }
            
            // Clear existing options except the first one
            vehicleSelect.innerHTML = '<option value="">Select a vehicle...</option>';
            
            // Fetch available vehicles from database using correct API structure
            const response = await fetch(`${this.apiBaseUrl}/vehicles`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $v isa vehicle, has vehicle-id $id, has vehicle-type $type, has status $status, has maintenance-status "good", has fuel-level $fuel; $fuel >= 50.0; not { $assignment isa assignment (assigned-vehicle: $v, assigned-employee: $employee); }; select $v, $id, $type, $status; limit 10;'
                })
            });
            
            if (!response.ok) {
                throw new Error(`Failed to fetch available vehicles: ${response.status} ${response.statusText}`);
            }
            
            const result = await response.json();
            console.log('Loaded available vehicles from database:', result);
            
            const availableVehicles = result.ok?.answers || [];
            
            availableVehicles.forEach(answer => {
                const vehicleData = answer.data;
                if (vehicleData && vehicleData.id) {
                    const option = document.createElement('option');
                    option.value = vehicleData.id.value;
                    option.textContent = `${vehicleData.id.value} - ${vehicleData.type?.value || 'Vehicle'} (${vehicleData.status?.value || 'available'})`;
                    vehicleSelect.appendChild(option);
                }
            });
            
            console.log(`Loaded ${availableVehicles.length} available vehicles from database`);
            
        } catch (error) {
            console.error('Error loading available vehicles:', error);
            
            // Show error in dropdown
            const vehicleSelect = document.getElementById('vehicle-assignment');
            if (vehicleSelect) {
                vehicleSelect.innerHTML = '<option value="">Error loading vehicles - check backend</option>';
            }
            
            this.showNotification('Failed to load available vehicles from database', 'error');
        }
    }
    
    setupDateDependentDriverLoading() {
        const scheduleInput = document.getElementById('assignment-schedule');
        const pickupAddressInput = document.getElementById('pickup-address');
        const driverContainer = document.getElementById('driver-assignment-container');
        const driverSelect = document.getElementById('driver-assignment');
        
        if (scheduleInput && pickupAddressInput && driverContainer && driverSelect) {
            const loadDriversIfReady = () => {
                const selectedDateTime = scheduleInput.value;
                const pickupLat = parseFloat(document.getElementById('pickup-lat').value);
                const pickupLng = parseFloat(document.getElementById('pickup-lng').value);
                const deliveryLat = parseFloat(document.getElementById('delivery-lat').value);
                const deliveryLng = parseFloat(document.getElementById('delivery-lng').value);
                
                if (selectedDateTime && !isNaN(pickupLat) && !isNaN(pickupLng) && !isNaN(deliveryLat) && !isNaN(deliveryLng)) {
                    console.log('Date, pickup and delivery locations available, loading drivers for:', selectedDateTime);
                    // Show the driver dropdown container
                    driverContainer.classList.remove('hidden');
                    this.loadAvailableDriversForDateAndLocation(selectedDateTime, pickupLat, pickupLng, deliveryLat, deliveryLng);
                } else {
                    // Hide driver dropdown if date or locations not selected
                    driverContainer.classList.add('hidden');
                    driverSelect.innerHTML = '<option value="">Select a driver...</option>';
                }
            };
            
            scheduleInput.addEventListener('change', loadDriversIfReady);
            
            // Also listen for address selection (when coordinates are set)
            const originalSelectAddress = this.selectAddress.bind(this);
            this.selectAddress = (address, lat, lng, inputId) => {
                originalSelectAddress(address, lat, lng, inputId);
                // Trigger driver loading after address is selected
                setTimeout(loadDriversIfReady, 100);
            };
        }
    }
    
    async loadAvailableDriversForDateAndLocation(assignmentDateTime, pickupLat, pickupLng, deliveryLat, deliveryLng) {
        try {
            const driverSelect = document.getElementById('driver-assignment');
            if (!driverSelect) {
                console.error('Driver select element not found');
                return;
            }
            
            // Clear existing options
            driverSelect.innerHTML = '<option value="">Loading drivers...</option>';
            
            // Convert datetime-local format to ISO format for query
            const assignmentDate = new Date(assignmentDateTime);
            const isoDateTime = assignmentDate.toISOString();
            
            // Detect jurisdiction based on coordinates
            const jurisdiction = this.detectJurisdiction(pickupLat, pickupLng, deliveryLat, deliveryLng);
            
            // Get jurisdiction-specific hours limit from rules service
            const maxHours = await this.getJurisdictionHoursLimit(jurisdiction);
            
            // Create bounding box for proximity (±0.1 degrees ≈ 11km radius)
            const deltaLat = 0.1;
            const deltaLng = 0.1;
            
            // Calculate bounding box that encompasses both pickup and delivery locations
            const allLats = [pickupLat, deliveryLat];
            const allLngs = [pickupLng, deliveryLng];
            const minLatBase = Math.min(...allLats);
            const maxLatBase = Math.max(...allLats);
            const minLngBase = Math.min(...allLngs);
            const maxLngBase = Math.max(...allLngs);
            
            // Expand bounding box by delta around the route area
            const minLat = minLatBase - deltaLat;
            const maxLat = maxLatBase + deltaLat;
            const minLng = minLngBase - deltaLng;
            const maxLng = maxLngBase + deltaLng;
            
            // Fetch available drivers for the specific date and location from database
            const response = await fetch(`${this.apiBaseUrl}/employees`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `match let $compliant in compliant_employees(${maxHours}); $compliant has id $id, has employee-name $name, has employee-role "driver", has daily-hours $hours, has performance-rating $rating, has certifications $certs; { $compliant has current-lat $lat, has current-lng $lng; not { $assignment1 isa assignment (assigned-employee: $compliant, assigned-delivery: $delivery1), has timestamp $assignTime1; $assignTime1 == "${isoDateTime}"; }; } or { $assignment2 isa assignment (assigned-employee: $compliant, assigned-delivery: $delivery2), has timestamp $assignTime2; $assignTime2 == "${isoDateTime}"; $delivery2 has dest-lat $lat, has dest-lng $lng; }; $lat > ${minLat}; $lat < ${maxLat}; $lng > ${minLng}; $lng < ${maxLng}; select $compliant, $id, $name, $lat, $lng, $hours, $rating, $certs; limit 15;`
                })
            });
            
            if (!response.ok) {
                throw new Error(`Failed to fetch available drivers: ${response.status} ${response.statusText}`);
            }
            
            const result = await response.json();
            console.log('Loaded available drivers for date and location:', result);
            
            const availableDrivers = result.ok?.answers || [];
            
            // Calculate comprehensive fleet assignment scores with TomTom routing
            const driversWithScores = await Promise.all(availableDrivers.map(async (answer) => {
                const driverData = answer.data;
                if (driverData && driverData.id && driverData.lat && driverData.lng) {
                    const driverLat = parseFloat(driverData.lat.value);
                    const driverLng = parseFloat(driverData.lng.value);
                    const dailyHours = parseFloat(driverData.hours?.value || 0);
                    const performanceRating = parseFloat(driverData.rating?.value || 3.0);
                    const driverCertifications = driverData.certs?.value || '';
                    
                    // Get required certifications from form
                    const requiredCerts = Array.from(document.querySelectorAll('.certification-checkbox:checked')).map(cb => cb.value);
                    const certificationMatch = this.calculateCertificationMatch(driverCertifications, requiredCerts);
                    
                    // Get actual vehicle data for this driver
                    const vehicleData = await this.getDriverVehicleData(driverData.id.value);
                    const fuelLevel = vehicleData.fuelLevel || 75;
                    const vehicleCapacity = vehicleData.capacity || 1000;
                    const vehicleId = vehicleData.vehicleId || 'VH' + driverData.id.value.slice(-3);
                    const vehicleType = vehicleData.vehicleType || 'truck';
                    
                    // Store coordinates for TomTom routing API calls
                    const routeCoordinates = {
                        driver: [driverLng, driverLat],
                        pickup: [pickupLng, pickupLat], 
                        delivery: [deliveryLng, deliveryLat]
                    };
                    
                    // Use TomTom routing for precise calculations
                    const routeData = await this.calculateTomTomRoute(routeCoordinates);
                    const pickupDistance = routeData.totalDistance || this.calculateDistance(pickupLat, pickupLng, driverLat, driverLng);
                    const deliveryDistance = this.calculateDistance(pickupLat, pickupLng, deliveryLat, deliveryLng);
                    const totalRouteDistance = routeData.totalDistance || (pickupDistance + deliveryDistance);
                    const estimatedTime = routeData.totalTime || ((pickupDistance + deliveryDistance) * 2);
                    
                    // Get traffic information for better routing decisions (with delay to prevent rate limiting)
                    await new Promise(resolve => setTimeout(resolve, Math.random() * 100)); // Random delay 0-100ms
                    const trafficData = await this.getTomTomTraffic(routeCoordinates);
                    const trafficDelay = trafficData.trafficDelay || 0;
                    
                    // Multi-objective scoring system with certification matching
                    const distanceScore = Math.max(0, 100 - (pickupDistance * 10)); // Closer = higher score
                    const hoursScore = Math.max(0, (11 - dailyHours) * 10); // More available hours = higher score
                    const performanceScore = performanceRating * 20; // Performance rating 1-5 -> 20-100
                    const fuelScore = Math.min(100, fuelLevel * 2); // Fuel level 0-100 -> 0-100
                    const capacityScore = Math.min(100, vehicleCapacity / 10); // Capacity bonus
                    const certificationScore = certificationMatch * 100; // Certification match 0-1 -> 0-100
                    const timeScore = Math.max(0, 100 - (estimatedTime * 2)); // Faster routes = higher score
                    
                    // Enhanced weighted composite score (distance 30%, hours 20%, performance 15%, time 15%, certs 10%, fuel 5%, capacity 5%)
                    const compositeScore = (distanceScore * 0.3) + (hoursScore * 0.2) + (performanceScore * 0.15) + (timeScore * 0.15) + (certificationScore * 0.1) + (fuelScore * 0.05) + (capacityScore * 0.05);
                    
                    return {
                        id: driverData.id.value,
                        name: driverData.name?.value || driverData.id.value,
                        vehicleId: 'VH' + driverData.id.value.slice(-3), // Generate vehicle ID
                        vehicleType: 'truck', // Default vehicle type
                        lat: driverLat,
                        lng: driverLng,
                        pickupDistance: pickupDistance,
                        deliveryDistance: deliveryDistance,
                        totalRouteDistance: totalRouteDistance,
                        dailyHours: dailyHours,
                        performanceRating: performanceRating,
                        fuelLevel: fuelLevel,
                        capacity: vehicleCapacity,
                        compositeScore: compositeScore,
                        distanceScore: distanceScore,
                        hoursScore: hoursScore,
                        performanceScore: performanceScore,
                        routeCoordinates: routeCoordinates,
                        estimatedTime: estimatedTime,
                        vehicleId: vehicleId,
                        vehicleType: vehicleType
                    };
                }
                return null;
            })).then(results => results.filter(driver => driver !== null));
            
            // Sort by composite score (best overall match first)
            driversWithScores.sort((a, b) => b.compositeScore - a.compositeScore);
            
            // Clear loading message
            driverSelect.innerHTML = '<option value="">Select a driver...</option>';
            
            driversWithScores.forEach(driver => {
                const option = document.createElement('option');
                option.value = driver.id;
                option.textContent = `${driver.name} (Score: ${driver.compositeScore.toFixed(0)}, ${driver.estimatedTime.toFixed(0)}min, ${driver.dailyHours.toFixed(1)}h, ${driver.vehicleType})`;
                driverSelect.appendChild(option);
            });
            
            if (driversWithScores.length === 0) {
                const noDriversOption = document.createElement('option');
                noDriversOption.value = '';
                noDriversOption.textContent = 'No drivers available';
                noDriversOption.disabled = true;
                driverSelect.appendChild(noDriversOption);
            }
            
            
        } catch (error) {
            console.error('Error loading available drivers for date and location:', error);
            
            // Show error in dropdown
            const driverSelect = document.getElementById('driver-assignment');
            if (driverSelect) {
                driverSelect.innerHTML = '<option value="">Error loading drivers - check backend</option>';
            }
            
            this.showNotification('Failed to load available drivers for selected date and location', 'error');
        }
    }
    
    calculateDistance(lat1, lng1, lat2, lng2) {
        // Haversine formula to calculate distance between two points
        const R = 3959; // Earth's radius in miles
        const dLat = (lat2 - lat1) * Math.PI / 180;
        const dLng = (lng2 - lng1) * Math.PI / 180;
        const a = Math.sin(dLat/2) * Math.sin(dLat/2) +
                Math.cos(lat1 * Math.PI / 180) * Math.cos(lat2 * Math.PI / 180) *
                Math.sin(dLng/2) * Math.sin(dLng/2);
        const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1-a));
        return R * c;
    }
    
    calculateCertificationMatch(driverCertifications, requiredCertifications) {
        // Calculate how well driver certifications match required certifications
        if (requiredCertifications.length === 0) {
            return 1.0; // Perfect match if no certifications required
        }
        
        const driverCertsList = driverCertifications.split(',').map(cert => cert.trim().toUpperCase());
        const requiredCertsList = requiredCertifications.map(cert => cert.toUpperCase());
        
        const matchedCerts = requiredCertsList.filter(reqCert => 
            driverCertsList.some(driverCert => driverCert.includes(reqCert))
        );
        
        return matchedCerts.length / requiredCertsList.length; // Return percentage match
    }
    
    async getDriverVehicleData(driverId) {
        // Fetch actual vehicle data for the driver
        try {
            const response = await fetch(`${this.apiBaseUrl}/vehicles`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `match $v isa vehicle, has vehicle-id $vehicleId, has fuel-level $fuel, has capacity $capacity, has vehicle-type $vType; $assignment isa assignment (assigned-vehicle: $v, assigned-employee: $e); $e has id "${driverId}"; select $v, $vehicleId, $fuel, $capacity, $vType; limit 1;`
                })
            });
            
            if (!response.ok) {
                console.warn(`Failed to fetch vehicle data for driver ${driverId}`);
                return {};
            }
            
            const result = await response.json();
            const vehicleAnswers = result.ok?.answers || [];
            
            if (vehicleAnswers.length > 0) {
                const vehicleData = vehicleAnswers[0].data;
                return {
                    vehicleId: vehicleData.vehicleId?.value || null,
                    fuelLevel: parseFloat(vehicleData.fuel?.value || 75),
                    capacity: parseFloat(vehicleData.capacity?.value || 1000),
                    vehicleType: vehicleData.vType?.value || 'truck'
                };
            }
            
            return {}; // Return empty object if no vehicle found
        } catch (error) {
            console.warn(`Error fetching vehicle data for driver ${driverId}:`, error);
            return {};
        }
    }
    
    async calculateTomTomRoute(coordinates) {
        // Use the existing TomTom backend service instead of direct API calls
        // coordinates: { driver: [lng, lat], pickup: [lng, lat], delivery: [lng, lat] }
        try {
            // Call the backend TomTom service for route calculation
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'route'
                },
                body: JSON.stringify({
                    from_lat: coordinates.driver[1],
                    from_lng: coordinates.driver[0],
                    to_lat: coordinates.pickup[1],
                    to_lng: coordinates.pickup[0],
                    vehicle_id: 'temp_vehicle',
                    delivery_id: 'temp_delivery'
                })
            });
            
            if (!response.ok) {
                throw new Error(`TomTom backend service failed: ${response.status}`);
            }
            
            const data = await response.json();
            
            if (data.status === 'success') {
                return {
                    totalDistance: data.distance_meters / 1609.34, // Convert to miles
                    totalTime: data.duration_seconds / 60, // Convert to minutes
                    trafficDelay: 0, // Would need traffic endpoint for this
                    fuelConsumption: 0
                };
            } else {
                throw new Error('Invalid response from TomTom service');
            }
        } catch (error) {
            console.warn('TomTom backend service failed, using Haversine fallback:', error);
            // Fallback to Haversine calculation
            const driverToPickup = this.calculateDistance(coordinates.driver[1], coordinates.driver[0], coordinates.pickup[1], coordinates.pickup[0]);
            const pickupToDelivery = this.calculateDistance(coordinates.pickup[1], coordinates.pickup[0], coordinates.delivery[1], coordinates.delivery[0]);
            
            return {
                totalDistance: driverToPickup + pickupToDelivery,
                totalTime: (driverToPickup + pickupToDelivery) * 2, // Rough estimate: 30 mph average
                trafficDelay: 0,
                fuelConsumption: 0
            };
        }
    }
    
    async getTomTomTraffic(coordinates) {
        // Use the existing TomTom backend service for traffic information
        try {
            // Validate coordinates before making API call
            if (!coordinates || !coordinates.driver || !coordinates.pickup ||
                typeof coordinates.driver[1] !== 'number' || typeof coordinates.driver[0] !== 'number' ||
                typeof coordinates.pickup[1] !== 'number' || typeof coordinates.pickup[0] !== 'number' ||
                isNaN(coordinates.driver[1]) || isNaN(coordinates.driver[0]) ||
                isNaN(coordinates.pickup[1]) || isNaN(coordinates.pickup[0])) {
                console.warn('Invalid coordinates for TomTom traffic service:', coordinates);
                return {
                    trafficDelay: 0,
                    congestionLevel: 'unknown',
                    travelTime: 0
                };
            }

            const requestBody = {
                from_lat: Number(coordinates.driver[1]),
                from_lng: Number(coordinates.driver[0]),
                to_lat: Number(coordinates.pickup[1]),
                to_lng: Number(coordinates.pickup[0]),
                vehicle_id: 'temp_vehicle'
            };

            console.log('TomTom traffic request:', requestBody);

            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'traffic'
                },
                body: JSON.stringify(requestBody)
            });
            
            if (!response.ok) {
                const errorText = await response.text();
                console.error('TomTom traffic service error response:', errorText);
                throw new Error(`TomTom traffic service failed: ${response.status} - ${errorText}`);
            }
            
            const data = await response.json();
            console.log('TomTom traffic response:', data);
            
            if (data.status === 'success') {
                return {
                    trafficDelay: (data.traffic_delay_seconds || 0) / 60, // Convert to minutes
                    congestionLevel: data.congestion_level || 'unknown',
                    travelTime: (data.travel_time_seconds || 0) / 60
                };
            } else {
                throw new Error(`Invalid response from TomTom traffic service: ${JSON.stringify(data)}`);
            }
        } catch (error) {
            console.warn('TomTom traffic service failed:', error);
            return {
                trafficDelay: 0,
                congestionLevel: 'unknown',
                travelTime: 0
            };
        }
    }
    
    setupFormValidation() {
        const validateForm = () => {
            const routeId = document.getElementById('route-id').value.trim();
            const vehicleId = document.getElementById('vehicle-assignment').value;
            const pickupAddress = document.getElementById('pickup-address').value.trim();
            const pickupLat = document.getElementById('pickup-lat').value;
            const pickupLng = document.getElementById('pickup-lng').value;
            const deliveryAddress = document.getElementById('delivery-address').value.trim();
            const deliveryLat = document.getElementById('delivery-lat').value;
            const deliveryLng = document.getElementById('delivery-lng').value;
            const deliveryPriority = document.getElementById('delivery-priority').value;
            const scheduleTime = document.getElementById('assignment-schedule').value;
            const driverId = document.getElementById('driver-assignment').value;
            
            const submitBtn = document.getElementById('schedule-assignment-btn');
            
            // Check if all required fields are filled
            const isValid = routeId && 
                           vehicleId && 
                           pickupAddress && 
                           pickupLat && 
                           pickupLng && 
                           deliveryAddress &&
                           deliveryLat &&
                           deliveryLng &&
                           deliveryPriority && 
                           scheduleTime && 
                           driverId;
            
            if (isValid) {
                // Enable button
                submitBtn.disabled = false;
                submitBtn.className = 'flex-1 bg-blue-500 text-white px-4 py-2 text-sm rounded-lg hover:bg-blue-600 transition-colors cursor-pointer';
            } else {
                // Disable button
                submitBtn.disabled = true;
                submitBtn.className = 'flex-1 bg-gray-400 text-white px-4 py-2 text-sm rounded-lg cursor-not-allowed transition-colors';
            }
        };
        
        // Add event listeners to all form fields
        const fields = [
            'route-id',
            'vehicle-assignment', 
            'pickup-address',
            'delivery-address',
            'delivery-priority',
            'assignment-schedule',
            'driver-assignment'
        ];
        
        fields.forEach(fieldId => {
            const field = document.getElementById(fieldId);
            if (field) {
                field.addEventListener('input', validateForm);
                field.addEventListener('change', validateForm);
            }
        });
        
        // Also validate when address coordinates are set
        const originalSelectAddress = this.selectAddress;
        this.selectAddress = (address, lat, lng, inputId) => {
            if (originalSelectAddress) {
                originalSelectAddress.call(this, address, lat, lng, inputId);
            }
            setTimeout(validateForm, 100);
        };
        
        // Initial validation
        validateForm();
    }
    
    async loadAvailableDrivers() {
        try {
            const driverSelect = document.getElementById('driver-assignment');
            if (!driverSelect) {
                console.error('Driver select element not found');
                return;
            }
            
            // Clear existing options except the first one
            driverSelect.innerHTML = '<option value="">Select a driver...</option>';
            
            // Fetch available drivers from database using correct API structure
            const response = await fetch(`${this.apiBaseUrl}/employees`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: 'match $e isa employee, has id $id, has employee-name $name, has employee-role "driver", has status "available", has daily-hours $hours; $hours < 11.0; not { $assignment isa assignment (assigned-employee: $e, assigned-vehicle: $vehicle); }; select $e, $id, $name; limit 10;'
                })
            });
            
            if (!response.ok) {
                throw new Error(`Failed to fetch available drivers: ${response.status} ${response.statusText}`);
            }
            
            const result = await response.json();
            const availableDrivers = result.ok?.answers || [];
            
            availableDrivers.forEach(answer => {
                const driverData = answer.data;
                if (driverData && driverData.id) {
                    const option = document.createElement('option');
                    option.value = driverData.id.value;
                    option.textContent = `${driverData.name?.value || driverData.id.value} (Driver)`;
                    driverSelect.appendChild(option);
                }
            });
            
        } catch (error) {
            console.error('Error loading available drivers:', error);
            
            // Show error in dropdown
            const errorSelect = document.getElementById('driver-assignment');
            if (errorSelect) {
                errorSelect.innerHTML = '<option value="">Error loading drivers - check backend</option>';
            }
            
            this.showNotification('Failed to load available drivers from database', 'error');
        }
    }
    
    setupAddressAutocomplete() {
        // Wait a bit for the DOM to be ready
        setTimeout(() => {
            this.setupSingleAddressAutocomplete('pickup-address', 'address-suggestions');
            this.setupSingleAddressAutocomplete('delivery-address', 'delivery-suggestions');
        }, 500);
    }
    
    setupSingleAddressAutocomplete(inputId, suggestionsId) {
        const addressInput = document.getElementById(inputId);
        const suggestionsContainer = document.getElementById(suggestionsId);
        let searchTimeout;
        
        if (!addressInput || !suggestionsContainer) {
            console.error(`Address input ${inputId} or suggestions container ${suggestionsId} not found`);
            return;
        }
            
            addressInput.addEventListener('input', (e) => {
                // Skip autocomplete if this is a programmatic update
                if (e.target._programmaticUpdate) {
                    return;
                }
                
                const query = e.target.value.trim();
                
                // Clear previous timeout
                if (searchTimeout) {
                    clearTimeout(searchTimeout);
                }
                
                if (query.length < 3) {
                    suggestionsContainer.classList.add('hidden');
                    return;
                }
                
                // Debounce the search
                searchTimeout = setTimeout(() => {
                    this.searchAddresses(query, suggestionsId, inputId);
                }, 300);
            });
            
            // Hide suggestions when clicking outside
            document.addEventListener('click', (e) => {
                if (!addressInput.contains(e.target) && !suggestionsContainer.contains(e.target)) {
                    suggestionsContainer.classList.add('hidden');
                }
            });
    }
    
    async searchAddresses(query, suggestionsId, inputId) {
        const suggestionsContainer = document.getElementById(suggestionsId);
        
        try {
            
            // Use TomTom backend service for address autocomplete
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'search'
                },
                body: JSON.stringify({
                    query: query
                })
            });
           
            
            if (!response.ok) {
                const errorText = await response.text();
                console.error('TomTom API error:', errorText);
                throw new Error(`Failed to fetch address suggestions: ${response.status}`);
            }
            
            const data = await response.json();
            console.log('TomTom backend results:', data);
            
            // Handle TomTom backend service response format for search
            if (data.status === 'success' && data.results) {
                this.displayAddressSuggestions(data.results, suggestionsId, inputId);
            } else {
                throw new Error('No search results found');
            }
            
        } catch (error) {
            console.error('Error fetching address suggestions:', error);
            suggestionsContainer.innerHTML = '<div class="px-4 py-3 text-sm text-red-600 text-center">Error loading suggestions</div>';
            suggestionsContainer.classList.remove('hidden');
        }
    }
    
    displayAddressSuggestions(results, suggestionsId, inputId) {
        const suggestionsContainer = document.getElementById(suggestionsId);
        
        if (results.length === 0) {
            suggestionsContainer.innerHTML = '<div class="px-4 py-3 text-sm text-gray-500 text-center">No addresses found</div>';
            suggestionsContainer.classList.remove('hidden');
            return;
        }
        
        const suggestionsHtml = results.map((result, index) => {
            const address = result.address.freeformAddress;
            const lat = result.position.lat;
            const lng = result.position.lon;
            
            return `
                <div class="px-4 py-3 hover:bg-blue-50 cursor-pointer border-b border-gray-100 last:border-b-0 transition-all duration-200 hover:shadow-sm" 
                     data-address="${encodeURIComponent(address)}" 
                     data-lat="${lat}" 
                     data-lng="${lng}"
                     onclick="fleetCommand.selectAddress('${address.replace(/'/g, "\\'")}', ${lat}, ${lng}, '${inputId}')">
                    <div class="text-sm font-medium text-gray-800 leading-relaxed">${address}</div>
                </div>
            `;
        }).join('');
        
        suggestionsContainer.innerHTML = suggestionsHtml;
        suggestionsContainer.classList.remove('hidden');
        
        // Ensure proper z-index and positioning with enhanced styling
        suggestionsContainer.style.zIndex = '1000';
        suggestionsContainer.style.position = 'absolute';
        suggestionsContainer.style.top = '100%';
        suggestionsContainer.style.left = '0';
        suggestionsContainer.style.right = '0';
        suggestionsContainer.style.marginTop = '4px';
        suggestionsContainer.style.borderRadius = '8px';
        suggestionsContainer.style.boxShadow = '0 10px 25px -5px rgba(0, 0, 0, 0.1), 0 10px 10px -5px rgba(0, 0, 0, 0.04)';
        suggestionsContainer.style.border = '1px solid #e5e7eb';
        suggestionsContainer.style.backgroundColor = '#ffffff';
    }
    
    selectAddress(address, lat, lng, inputId = 'pickup-address') {
       
        const isDelivery = inputId === 'delivery-address';
        const addressInput = document.getElementById(inputId);
        const latInput = document.getElementById(isDelivery ? 'delivery-lat' : 'pickup-lat');
        const lngInput = document.getElementById(isDelivery ? 'delivery-lng' : 'pickup-lng');
        const suggestionsContainer = document.getElementById(isDelivery ? 'delivery-suggestions' : 'address-suggestions');
        
        if (addressInput && latInput && lngInput && suggestionsContainer) {
            // Set flag to prevent autocomplete from running
            addressInput._programmaticUpdate = true;
            
            // Force the values and trigger change events
            addressInput.value = address;
            latInput.value = lat;
            lngInput.value = lng;
            
            // Hide suggestions immediately
            suggestionsContainer.classList.add('hidden');
            
            // Trigger change events to ensure form validation updates
            addressInput.dispatchEvent(new Event('change', { bubbles: true }));
            
            // Clear the flag after a short delay
            setTimeout(() => {
                delete addressInput._programmaticUpdate;
            }, 100);
            
        } else {
            console.error('❌ Missing elements for address selection');
        }
    }
    
    detectJurisdiction(pickupLat, pickupLng, deliveryLat, deliveryLng) {
        // Simple jurisdiction detection based on coordinate ranges
        // This could be enhanced with more sophisticated geofencing
        
        // Use pickup location as primary jurisdiction determinant
        const lat = pickupLat;
        const lng = pickupLng;
        
        // North America (US/Canada)
        if (lat >= 25 && lat <= 72 && lng >= -168 && lng <= -52) {
            if (lat >= 49) {
                return 'CA'; // Canada
            }
            return 'US'; // United States
        }
        
        // Europe
        if (lat >= 35 && lat <= 71 && lng >= -10 && lng <= 40) {
            return 'EU'; // European Union
        }
        
        // South America (Chile example from logs)
        if (lat >= -56 && lat <= 12 && lng >= -82 && lng <= -34) {
            return 'CL'; // Chile
        }
        
        // Australia/Oceania
        if (lat >= -47 && lat <= -10 && lng >= 113 && lng <= 154) {
            return 'AU'; // Australia
        }
        
        // Default to international standards
        return 'INTL';
    }
    
    async getJurisdictionHoursLimit(jurisdiction) {
        try {
            // Fetch hours limit from rules service (try jurisdiction-specific first, then general employee rule)
            let response = await fetch(`${this.apiBaseUrl}/rules`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `match $rule isa rule, has rule-name "max_daily_hours.${jurisdiction}", has rule-value $value; select $value;`
                })
            });
            
            // If no jurisdiction-specific rule found, try general employee rule
            let result;
            if (response.ok) {
                result = await response.json();
                if (!result.ok?.answers || result.ok.answers.length === 0) {
                    response = await fetch(`${this.apiBaseUrl}/rules`, {
                        method: 'POST',
                        headers: {
                            'Content-Type': 'application/json',
                            'x-service-method': 'read'
                        },
                        body: JSON.stringify({
                            query: `match $rule isa rule, has rule-name "employee.max_daily_hours", has rule-value $value; select $value;`
                        })
                    });
                    
                    if (response.ok) {
                        result = await response.json();
                    }
                }
            }
            
            if (!response.ok) {
                console.warn(`Failed to fetch hours limit for jurisdiction ${jurisdiction}, using default`);
                return this.getDefaultHoursLimit(jurisdiction);
            }
            const rules = result.ok?.answers || [];
            
            if (rules.length > 0 && rules[0].data?.value?.value) {
                const hoursLimit = parseFloat(rules[0].data.value.value);
                console.log(`📋 Jurisdiction ${jurisdiction} hours limit: ${hoursLimit}`);
                return hoursLimit;
            }
            
            // Fallback to default if no rule found
            return this.getDefaultHoursLimit(jurisdiction);
            
        } catch (error) {
            console.error('Error fetching jurisdiction hours limit:', error);
            return this.getDefaultHoursLimit(jurisdiction);
        }
    }
    
    getDefaultHoursLimit(jurisdiction) {
        // Regulatory defaults by jurisdiction
        const defaults = {
            'US': 11.0,    // US DOT: 11-hour driving limit
            'CA': 13.0,    // Canada: 13-hour on-duty limit  
            'EU': 9.0,     // EU: 9-hour daily driving limit
            'CL': 10.0,    // Chile: 10-hour limit
            'AU': 12.0,    // Australia: 12-hour work day
            'INTL': 10.0   // International default
        };
        
        const limit = defaults[jurisdiction] || defaults['INTL'];
        console.log(`📋 Using default hours limit for ${jurisdiction}: ${limit}`);
        return limit;
    }
    
    setupDateTimeConstraints() {
        setTimeout(() => {
            const scheduleInput = document.getElementById('assignment-schedule');
            if (scheduleInput) {
                // Set minimum date/time to current date/time
                const now = new Date();
                // Format to YYYY-MM-DDTHH:MM format required by datetime-local
                const year = now.getFullYear();
                const month = String(now.getMonth() + 1).padStart(2, '0');
                const day = String(now.getDate()).padStart(2, '0');
                const hours = String(now.getHours()).padStart(2, '0');
                const minutes = String(now.getMinutes()).padStart(2, '0');
                
                const minDateTime = `${year}-${month}-${day}T${hours}:${minutes}`;
                scheduleInput.setAttribute('min', minDateTime);
                
                console.log('Set minimum datetime to:', minDateTime);
            } else {
                console.error('Schedule input not found for datetime constraints');
            }
        }, 100);
    }
    
    async loadRecentAssignments() {
        try {
            const response = await fetch(`${this.apiBaseUrl}/api/assignments/recent`);
            if (response.ok) {
                const assignments = await response.json();
                this.displayRecentAssignments(assignments);
            }
        } catch (error) {
            console.error('Error loading recent assignments:', error);
        }
    }
    
    displayRecentAssignments(assignments) {
        const container = document.getElementById('recent-assignments');
        if (!container) return;
        
        if (assignments.length === 0) {
            container.innerHTML = '<div class="text-xs text-slate-500">No recent assignments</div>';
            return;
        }
        
        container.innerHTML = assignments.map(assignment => `
            <div class="bg-slate-50 rounded p-2">
                <div class="text-xs font-medium">${assignment.route_id}</div>
                <div class="text-xs text-slate-600">${assignment.delivery_priority} priority</div>
                <div class="text-xs text-slate-500">${new Date(assignment.created_at).toLocaleDateString()}</div>
            </div>
        `).join('');
    }
    
    showNotification(message, type = 'info') {
        const notification = document.createElement('div');
        notification.className = `fixed top-4 right-4 z-50 p-4 rounded-lg shadow-lg text-white ${
            type === 'success' ? 'bg-green-500' : 
            type === 'error' ? 'bg-red-500' : 'bg-blue-500'
        }`;
        notification.textContent = message;
        
        document.body.appendChild(notification);
        
        setTimeout(() => {
            notification.remove();
        }, 3000);
    }
    
}

// Initialize FleetCommand Pro when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    console.log('🚀 Starting FleetCommand Pro Enterprise System...');
    window.fleetCommand = new FleetCommandPro();
});
