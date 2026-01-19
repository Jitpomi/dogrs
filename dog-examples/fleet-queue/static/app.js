class FleetCommandPro {
    constructor() {
        this.apiBaseUrl = 'http://127.0.0.1:3036';
        this.tomtomApiKey = process.env.TOMTOM_API_KEY;
        this.map = null;
        this.vehicleMarkers = new Map();
        this.deliveryMarkers = new Map();
        this.assignmentRoutes = new Map();
        this.trackedRoute = null;
        this.trackedDeliveryMarker = null;
        this.trackedVehicleId = null;
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
                    query: 'match $v isa vehicle, has vehicle-id $id, has vehicle-type $type, has vehicle-status $status, has vehicle-icon $icon, has status-color $color, has gps-latitude $lat, has gps-longitude $lng, has capacity $capacity, has fuel-level $fuel, has maintenance-score $maintenance; $assignment isa assignment (assigned-vehicle: $v, assigned-employee: $employee); select $v, $id, $type, $status, $icon, $color, $lat, $lng, $capacity, $fuel, $maintenance; limit 50;'
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
                    query: 'match $op isa operation-event; select $op; limit 50;'
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
                @keyframes slideInFromRight {
                    from { transform: translateX(100%); opacity: 0; }
                    to { transform: translateX(0); opacity: 1; }
                }
                @keyframes slideOutToRight {
                    from { transform: translateX(0); opacity: 1; }
                    to { transform: translateX(100%); opacity: 0; }
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
                                        <span class="text-slate-900 font-medium text-sm driver-name">${driverName}</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Status:</span>
                                        <span class="px-2 py-1 text-xs font-medium rounded-full capitalize text-white driver-status" style="background: ${statusColor};">${driverStatus}</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Rating:</span>
                                        <span class="text-slate-900 font-medium text-sm driver-rating">${driverRating}/5 ⭐</span>
                                    </div>
                                    <div class="flex justify-between items-center">
                                        <span class="text-slate-600 text-sm">Certifications:</span>
                                        <span class="text-slate-900 font-medium text-sm driver-certs">${driverCerts}</span>
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
                            <div class="flex gap-3" id="vehicle-actions-${vehicleId}">
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
                // Load current driver data when popup opens
                setTimeout(() => this.loadDriverDataForPopup(vehicleId), 150);
            });
            
            // Add intelligent popup positioning and map centering
            marker.on('click', () => {
                // Center the map on the selected vehicle
                this.centerMapOnVehicle(vehicleId, [lng, lat]);
                // Then ensure popup is in view
                setTimeout(() => this.ensurePopupInView(vehicleId, [lng, lat]), 300);
                // Load traffic data for this vehicle
                setTimeout(() => this.loadVehicleTrafficData(vehicleId, lat, lng), 500);
                // Load current driver data for this vehicle
                setTimeout(() => this.loadDriverDataForPopup(vehicleId), 550);
            });
        });
        
        // Wait for all vehicle markers to be processed
        await Promise.all(vehiclePromises);
        
        
        // Center map on all vehicles after all markers are added
        this.centerMapOnVehicles();
    }
    
    async addDeliveryMarkers() {
        if (!this.map) return;
        
        console.log('🚚 Starting addDeliveryMarkers...');
        
        // Clear existing delivery markers
        this.deliveryMarkers.forEach(marker => marker.remove());
        this.deliveryMarkers.clear();
        
        try {
            // Get all deliveries with their coordinates
            const response = await fetch(`${this.apiBaseUrl}/deliveries`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `
                        match 
                        $delivery isa delivery,
                            has delivery-id $deliveryId,
                            has customer-name $customer,
                            has delivery-address $address,
                            has dest-lat $lat,
                            has dest-lng $lng,
                            has delivery-status $status,
                            has customer-priority $priority;
                        select $deliveryId, $customer, $address, $lat, $lng, $status, $priority;
                    `
                })
            });
            
            if (response.ok) {
                const data = await response.json();
                if (data.ok && data.ok.answers) {
                    console.log(`📦 Found ${data.ok.answers.length} deliveries to display`);
                    data.ok.answers.forEach(delivery => {
                        const deliveryData = delivery.data;
                        const deliveryId = deliveryData.deliveryId?.value;
                        const customer = deliveryData.customer?.value;
                        const address = deliveryData.address?.value;
                        const lat = parseFloat(deliveryData.lat?.value);
                        const lng = parseFloat(deliveryData.lng?.value);
                        const status = deliveryData.status?.value;
                        const priority = deliveryData.priority?.value;
                        
                        console.log(`📍 Creating delivery marker: ${deliveryId} at (${lat}, ${lng})`);
                        
                        if (deliveryId && lat && lng) {
                            this.createDeliveryMarker(deliveryId, customer, address, lat, lng, status, priority);
                        } else {
                            console.warn(`⚠️ Skipping delivery ${deliveryId}: missing coordinates`);
                        }
                    });
                }
            }
        } catch (error) {
            console.error('Error loading delivery markers:', error);
            console.error('Delivery markers error details:', error.message, error.stack);
        }
    }
    
    createDeliveryMarker(deliveryId, customer, address, lat, lng, status, priority) {
        const statusColors = {
            'pending': '#f59e0b',
            'in_transit': '#3b82f6',
            'delivered': '#10b981',
            'cancelled': '#ef4444'
        };
        
        const priorityIcons = {
            'standard': '📦',
            'express': '⚡',
            'urgent': '🚨',
            'critical': '🔥'
        };
        
        const color = statusColors[status] || '#6b7280';
        const icon = priorityIcons[priority] || '📦';
        
        const markerElement = document.createElement('div');
        markerElement.className = 'delivery-marker';
        markerElement.innerHTML = `
            <div style="
                width: 28px; 
                height: 28px; 
                background: linear-gradient(135deg, ${color}, ${this.darkenColor(color, 0.2)});
                border: 2px solid #ffffff;
                border-radius: 50%;
                display: flex;
                align-items: center;
                justify-content: center;
                box-shadow: 0 4px 12px rgba(0,0,0,0.3);
                cursor: pointer;
                font-size: 12px;
                transition: all 0.3s ease;
            " onmouseover="this.style.transform='scale(1.2)'; this.style.zIndex='1000';" 
               onmouseout="this.style.transform='scale(1)'; this.style.zIndex='auto';">
                ${icon}
            </div>
        `;
        
        const marker = new tt.Marker({ element: markerElement })
            .setLngLat([lng, lat])
            .addTo(this.map);
        
        const popup = new tt.Popup({ 
            offset: 25,
            className: 'custom-popup',
            closeOnClick: false,
            closeButton: false
        })
        .setHTML(`
            <div class="delivery-popup-container" style="width: 300px;">
                <div class="flex items-center gap-3 mb-4">
                    <div style="background-color: ${color}; width: 20px; height: 20px; display: flex; align-items: center; justify-content: center; border-radius: 50%;">
                        <span class="text-white text-xs">${icon}</span>
                    </div>
                    <div>
                        <h3 class="text-sm font-semibold text-slate-900">${deliveryId}</h3>
                        <p class="text-xs text-slate-600">${customer}</p>
                    </div>
                </div>
                
                <div class="space-y-2 text-xs">
                    <div class="flex justify-between">
                        <span class="text-slate-600">Address:</span>
                        <span class="text-slate-900 font-medium text-right" style="max-width: 180px;">${address}</span>
                    </div>
                    <div class="flex justify-between">
                        <span class="text-slate-600">Status:</span>
                        <span class="px-2 py-1 text-xs font-medium rounded-full text-white" style="background: ${color};">${status}</span>
                    </div>
                    <div class="flex justify-between">
                        <span class="text-slate-600">Priority:</span>
                        <span class="text-slate-900 font-medium">${priority}</span>
                    </div>
                    <div class="flex justify-between">
                        <span class="text-slate-600">Coordinates:</span>
                        <span class="text-slate-900 font-mono text-xs">${lat.toFixed(4)}, ${lng.toFixed(4)}</span>
                    </div>
                </div>
            </div>
        `);
        
        marker.setPopup(popup);
        this.deliveryMarkers.set(deliveryId, marker);
    }
    
    async addAssignmentRoutes() {
        if (!this.map) return;
        
        console.log('🛣️ Starting addAssignmentRoutes...');
        
        // Clear existing assignment routes - remove layers before sources
        this.assignmentRoutes.forEach(route => {
            if (route.layer && this.map.getLayer(route.layer)) {
                this.map.removeLayer(route.layer);
            }
            if (route.source && this.map.getSource(route.source)) {
                this.map.removeSource(route.source);
            }
        });
        this.assignmentRoutes.clear();
        
        try {
            // Get all active assignments with vehicle and delivery coordinates
            const response = await fetch(`${this.apiBaseUrl}/operations`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `
                        match 
                        $assignment (assigned-employee: $employee, assigned-vehicle: $vehicle, assigned-delivery: $delivery) isa assignment,
                            has assignment-status $status;
                        $vehicle has vehicle-id $vehicleId, has gps-latitude $vLat, has gps-longitude $vLng;
                        $delivery has delivery-id $deliveryId, has dest-lat $dLat, has dest-lng $dLng;
                        $status == "active";
                        select $vehicleId, $deliveryId, $vLat, $vLng, $dLat, $dLng;
                    `
                })
            });
            
            if (response.ok) {
                const data = await response.json();
                console.log('🔍 Assignment routes response:', data);
                if (data.ok && data.ok.answers) {
                    console.log(`🚛 Found ${data.ok.answers.length} active assignments to display`);
                    data.ok.answers.forEach((assignment, index) => {
                        const assignmentData = assignment.data;
                        const vehicleId = assignmentData.vehicleId?.value;
                        const deliveryId = assignmentData.deliveryId?.value;
                        const vLat = parseFloat(assignmentData.vLat?.value);
                        const vLng = parseFloat(assignmentData.vLng?.value);
                        const dLat = parseFloat(assignmentData.dLat?.value);
                        const dLng = parseFloat(assignmentData.dLng?.value);
                        
                        console.log(`🗺️ Creating route: ${vehicleId} (${vLat}, ${vLng}) → ${deliveryId} (${dLat}, ${dLng})`);
                        
                        if (vehicleId && deliveryId && vLat && vLng && dLat && dLng) {
                            // Add small delay between route calculations to prevent API rate limiting
                            setTimeout(() => {
                                this.createAssignmentRoute(vehicleId, deliveryId, vLat, vLng, dLat, dLng, index);
                            }, index * 200); // 200ms delay between each route calculation
                        } else {
                            console.warn(`⚠️ Skipping assignment route: missing data for ${vehicleId} → ${deliveryId}`);
                        }
                    });
                } else {
                    console.log('📭 No active assignments found');
                }
            } else {
                console.error('❌ Assignment routes request failed:', response.status, response.statusText);
            }
        } catch (error) {
            console.error('Error loading assignment routes:', error);
        }
    }
    
    async createAssignmentRoute(vehicleId, deliveryId, vLat, vLng, dLat, dLng, index) {
        const sourceId = `assignment-route-${index}`;
        const layerId = `assignment-line-${index}`;
        
        console.log(`🛣️ Calculating TomTom route for assignment: ${vehicleId} → ${deliveryId}`);
        
        try {
            // Use TomTom route calculation API for actual road-based routes
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'route'
                },
                body: JSON.stringify({
                    from_lat: vLat,
                    from_lng: vLng,
                    to_lat: dLat,
                    to_lng: dLng
                })
            });
            
            if (response.ok) {
                const routeData = await response.json();
                
                let routeGeoJSON;
                
                if (routeData.status === 'success' && routeData.route_geometry && routeData.route_geometry.coordinates) {
                    const coordinates = routeData.route_geometry.coordinates;
                    
                    routeGeoJSON = {
                        type: 'Feature',
                        geometry: {
                            type: 'LineString',
                            coordinates: coordinates
                        },
                        properties: {
                            vehicleId: vehicleId,
                            deliveryId: deliveryId,
                            distance: routeData.distance_meters || 0,
                            duration: routeData.duration_seconds || 0
                        }
                    };
                    console.log(`✅ TomTom assignment route calculated: ${coordinates.length} points, ${(routeData.distance_meters / 1000).toFixed(1)}km`);
                } else {
                    console.warn(`⚠️ TomTom route failed for ${vehicleId} → ${deliveryId}, falling back to straight line`);
                    routeGeoJSON = this.createStraightLineRoute(vehicleId, deliveryId, vLat, vLng, dLat, dLng);
                }
                
                // Wait for map to be fully loaded before adding source and layer
                if (this.map.isStyleLoaded()) {
                    this.addRouteToMap(sourceId, layerId, routeGeoJSON, vehicleId, deliveryId);
                } else {
                    this.map.on('styledata', () => {
                        this.addRouteToMap(sourceId, layerId, routeGeoJSON, vehicleId, deliveryId);
                    });
                }
                
            } else {
                throw new Error(`TomTom route API failed: ${response.status}`);
            }
            
        } catch (error) {
            console.error(`❌ TomTom route calculation failed for ${vehicleId} → ${deliveryId}: ${error.message}`);
            console.log('📍 Falling back to straight line route');
            
            // Fallback to straight line if TomTom fails
            const routeGeoJSON = this.createStraightLineRoute(vehicleId, deliveryId, vLat, vLng, dLat, dLng);
            
            if (this.map.isStyleLoaded()) {
                this.addRouteToMap(sourceId, layerId, routeGeoJSON, vehicleId, deliveryId);
            } else {
                this.map.on('styledata', () => {
                    this.addRouteToMap(sourceId, layerId, routeGeoJSON, vehicleId, deliveryId);
                });
            }
        }
    }
    
    createStraightLineRoute(vehicleId, deliveryId, vLat, vLng, dLat, dLng) {
        return {
            type: 'Feature',
            geometry: {
                type: 'LineString',
                coordinates: [
                    [vLng, vLat], // Vehicle position
                    [dLng, dLat]  // Delivery destination
                ]
            },
            properties: {
                vehicleId: vehicleId,
                deliveryId: deliveryId,
                fallback: true
            }
        };
    }
    
    addRouteToMap(sourceId, layerId, routeGeoJSON, vehicleId, deliveryId) {
        try {
            // Add source if it doesn't exist
            if (!this.map.getSource(sourceId)) {
                this.map.addSource(sourceId, {
                    type: 'geojson',
                    data: routeGeoJSON
                });
            }
            
            // Add layer if it doesn't exist
            if (!this.map.getLayer(layerId)) {
                this.map.addLayer({
                    id: layerId,
                    type: 'line',
                    source: sourceId,
                    layout: {
                        'line-join': 'round',
                        'line-cap': 'round'
                    },
                    paint: {
                        'line-color': '#3b82f6',
                        'line-width': 3,
                        'line-opacity': 0.7,
                        'line-dasharray': [2, 2]
                    }
                });
                
                // Add click handler for route
                this.map.on('click', layerId, (e) => {
                    const coordinates = e.lngLat;
                    new tt.Popup()
                        .setLngLat(coordinates)
                        .setHTML(`
                            <div class="text-sm">
                                <div class="font-semibold mb-2">Active Assignment</div>
                                <div class="space-y-1">
                                    <div><span class="text-slate-600">Vehicle:</span> ${vehicleId}</div>
                                    <div><span class="text-slate-600">Delivery:</span> ${deliveryId}</div>
                                    <div><span class="text-slate-600">Status:</span> <span class="text-blue-600">En Route</span></div>
                                </div>
                            </div>
                        `)
                        .addTo(this.map);
                });
                
                // Change cursor on hover
                this.map.on('mouseenter', layerId, () => {
                    this.map.getCanvas().style.cursor = 'pointer';
                });
                
                this.map.on('mouseleave', layerId, () => {
                    this.map.getCanvas().style.cursor = '';
                });
            }
            
            // Store route info for cleanup
            this.assignmentRoutes.set(`${vehicleId}-${deliveryId}`, {
                source: sourceId,
                layer: layerId
            });
            
        } catch (error) {
            console.error('Error adding route to map:', error);
        }
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
                        $assignment has assigned-at $timestamp,
                                   has assignment-status "active";
                        $delivery has route-id $routeId,
                                 has pickup-address $pickup,
                                 has delivery-address $destination,
                                 has delivery-time $deliveryTime,
                                 has delivery-status $deliveryStatus,
                                 has dest-lat $destLat,
                                 has dest-lng $destLng;
                        select $routeId, $pickup, $destination, $deliveryTime, $deliveryStatus, $timestamp, $destLat, $destLng;
                        sort $deliveryTime asc;
                        limit 2;
                    `
                })
            });
            
            const routeData = await response.json();
            
            if (routeData.ok && routeData.ok.answers && routeData.ok.answers.length > 0) {
                const currentRoute = routeData.ok.answers[0].data;
                const routeId = currentRoute.routeId?.value || 'Unknown';
                const destination = currentRoute.destination?.value || 'Unknown destination';
                const deliveryTime = currentRoute.deliveryTime?.value || '';
                const deliveryStatus = currentRoute.deliveryStatus?.value || 'pending';
                
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
                
                // Update UI with current route data
                routeStatus.textContent = `${routeId} → ${destination.split(',')[0]}`;
                trafficStatus.innerHTML = `<span style="color: ${trafficConditions.color};">${trafficConditions.condition}</span>`;
                etaElement.textContent = eta;
                
                // Create dual route visualization
                this.createDualRouteVisualization(vehicleId, lat, lng, routeData.ok.answers);
                
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

    async createDualRouteVisualization(vehicleId, vehicleLat, vehicleLng, deliveries) {
        try {
            // Clear existing routes for this vehicle
            this.clearVehicleRoutes(vehicleId);
            
            if (deliveries.length === 0) return;
            
            // Current route (first delivery)
            const currentDelivery = deliveries[0].data;
            const currentDestLat = parseFloat(currentDelivery.destLat?.value);
            const currentDestLng = parseFloat(currentDelivery.destLng?.value);
            const currentRouteId = currentDelivery.routeId?.value;
            
            if (currentDestLat && currentDestLng) {
                await this.createVehicleRoute(
                    vehicleId, 
                    currentRouteId, 
                    vehicleLat, 
                    vehicleLng, 
                    currentDestLat, 
                    currentDestLng,
                    'current'
                );
            }
            
            // Next route (second delivery if exists)
            if (deliveries.length > 1) {
                const nextDelivery = deliveries[1].data;
                const nextDestLat = parseFloat(nextDelivery.destLat?.value);
                const nextDestLng = parseFloat(nextDelivery.destLng?.value);
                const nextRouteId = nextDelivery.routeId?.value;
                
                if (nextDestLat && nextDestLng) {
                    // For next route, start from current delivery destination
                    await this.createVehicleRoute(
                        vehicleId, 
                        nextRouteId, 
                        currentDestLat, 
                        currentDestLng, 
                        nextDestLat, 
                        nextDestLng,
                        'next'
                    );
                }
            }
            
        } catch (error) {
            console.error(`Error creating dual route visualization for ${vehicleId}:`, error);
        }
    }

    clearVehicleRoutes(vehicleId) {
        // Clear existing current and next routes for this vehicle
        const routeTypes = ['current', 'next'];
        routeTypes.forEach(type => {
            const sourceId = `${vehicleId}-${type}-route`;
            const layerId = `${vehicleId}-${type}-route-line`;
            
            if (this.map.getLayer(layerId)) {
                this.map.removeLayer(layerId);
            }
            if (this.map.getSource(sourceId)) {
                this.map.removeSource(sourceId);
            }
        });
    }

    async createVehicleRoute(vehicleId, routeId, fromLat, fromLng, toLat, toLng, routeType) {
        const sourceId = `${vehicleId}-${routeType}-route`;
        const layerId = `${vehicleId}-${routeType}-route-line`;
        
        try {
            // Use TomTom route calculation API
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'route'
                },
                body: JSON.stringify({
                    from_lat: fromLat,
                    from_lng: fromLng,
                    to_lat: toLat,
                    to_lng: toLng
                })
            });
            
            let routeGeoJSON;
            
            if (response.ok) {
                const routeData = await response.json();
                
                if (routeData.status === 'success' && routeData.route_geometry && routeData.route_geometry.coordinates) {
                    const coordinates = routeData.route_geometry.coordinates;
                    
                    routeGeoJSON = {
                        type: 'Feature',
                        geometry: {
                            type: 'LineString',
                            coordinates: coordinates
                        },
                        properties: {
                            vehicleId: vehicleId,
                            routeId: routeId,
                            routeType: routeType,
                            distance: routeData.distance_meters || 0,
                            duration: routeData.duration_seconds || 0
                        }
                    };
                } else {
                    // Fallback to straight line
                    routeGeoJSON = this.createStraightLineRoute(vehicleId, routeId, fromLat, fromLng, toLat, toLng);
                    routeGeoJSON.properties.routeType = routeType;
                }
            } else {
                // Fallback to straight line
                routeGeoJSON = this.createStraightLineRoute(vehicleId, routeId, fromLat, fromLng, toLat, toLng);
                routeGeoJSON.properties.routeType = routeType;
            }
            
            // Add route to map with different styling based on type
            this.addDualRouteToMap(sourceId, layerId, routeGeoJSON, routeType);
            
        } catch (error) {
            console.error(`Error creating ${routeType} route for ${vehicleId}:`, error);
            
            // Fallback to straight line
            const routeGeoJSON = this.createStraightLineRoute(vehicleId, routeId, fromLat, fromLng, toLat, toLng);
            routeGeoJSON.properties.routeType = routeType;
            this.addDualRouteToMap(sourceId, layerId, routeGeoJSON, routeType);
        }
    }

    addDualRouteToMap(sourceId, layerId, routeGeoJSON, routeType) {
        try {
            // Add source if it doesn't exist
            if (!this.map.getSource(sourceId)) {
                this.map.addSource(sourceId, {
                    type: 'geojson',
                    data: routeGeoJSON
                });
            }
            
            // Define styling based on route type
            const routeStyles = {
                current: {
                    color: '#3b82f6',      // Blue for current route
                    width: 4,
                    opacity: 0.8,
                    dasharray: null        // Solid line
                },
                next: {
                    color: '#10b981',      // Green for next route
                    width: 3,
                    opacity: 0.6,
                    dasharray: [4, 4]      // Dashed line
                }
            };
            
            const style = routeStyles[routeType] || routeStyles.current;
            
            // Add layer if it doesn't exist
            if (!this.map.getLayer(layerId)) {
                const layerConfig = {
                    id: layerId,
                    type: 'line',
                    source: sourceId,
                    layout: {
                        'line-join': 'round',
                        'line-cap': 'round'
                    },
                    paint: {
                        'line-color': style.color,
                        'line-width': style.width,
                        'line-opacity': style.opacity
                    }
                };
                
                if (style.dasharray) {
                    layerConfig.paint['line-dasharray'] = style.dasharray;
                }
                
                this.map.addLayer(layerConfig);
                
                // Add click handler for route
                this.map.on('click', layerId, (e) => {
                    const coordinates = e.lngLat;
                    const properties = routeGeoJSON.properties;
                    
                    new tt.Popup()
                        .setLngLat(coordinates)
                        .setHTML(`
                            <div class="text-sm">
                                <div class="font-semibold mb-2">${routeType === 'current' ? 'Current Route' : 'Next Route'}</div>
                                <div class="space-y-1">
                                    <div><span class="text-slate-600">Vehicle:</span> ${properties.vehicleId}</div>
                                    <div><span class="text-slate-600">Route:</span> ${properties.routeId}</div>
                                    <div><span class="text-slate-600">Type:</span> <span class="capitalize text-${routeType === 'current' ? 'blue' : 'green'}-600">${routeType}</span></div>
                                    <div><span class="text-slate-600">Distance:</span> ${(properties.distance / 1000).toFixed(1)}km</div>
                                </div>
                            </div>
                        `)
                        .addTo(this.map);
                });
                
                // Change cursor on hover
                this.map.on('mouseenter', layerId, () => {
                    this.map.getCanvas().style.cursor = 'pointer';
                });
                
                this.map.on('mouseleave', layerId, () => {
                    this.map.getCanvas().style.cursor = '';
                });
            }
            
        } catch (error) {
            console.error(`Error adding ${routeType} route to map:`, error);
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
                bottom: 20px;
                right: 20px;
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
            animation: slideInFromRight 0.3s ease-out;
            pointer-events: auto;
        `;
        notification.textContent = message;

        notificationContainer.appendChild(notification);

        // Auto-remove after 3 seconds
        setTimeout(() => {
            if (notification.parentElement) {
                notification.style.animation = 'slideOutToRight 0.3s ease-in';
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
    async trackVehicle(vehicleId) {
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
        
        // Set tracking mode without clearing existing routes
        this.enterTrackingMode(vehicleId);
        
        // Refresh popup route data to show current tracking assignment
        this.loadVehicleRouteData(vehicleId);
    }

    async displayVehicleRoute(vehicleId) {
        if (!this.map) return;
        
        console.log(`🛣️ Displaying route for vehicle ${vehicleId}`);
        
        try {
            // Get assignment for this specific vehicle using the working operations endpoint
            const response = await fetch(`${this.apiBaseUrl}/operations`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `
                        match 
                        $assignment (assigned-employee: $employee, assigned-vehicle: $vehicle, assigned-delivery: $delivery) isa assignment,
                            has assignment-status $status;
                        $vehicle has vehicle-id $vehicleId, has gps-latitude $vLat, has gps-longitude $vLng;
                        $delivery has delivery-id $deliveryId, has dest-lat $dLat, has dest-lng $dLng;
                        $status == "active";
                        select $vehicleId, $deliveryId, $vLat, $vLng, $dLat, $dLng;
                    `
                })
            });
            
            if (response.ok) {
                const data = await response.json();
                console.log(`🔍 Vehicle ${vehicleId} assignment response:`, data);
                
                if (data.ok && data.ok.answers && data.ok.answers.length > 0) {
                    // Find the assignment for this specific vehicle
                    const vehicleAssignment = data.ok.answers.find(assignment => {
                        const assignmentData = assignment.data;
                        return assignmentData.vehicleId?.value === vehicleId;
                    });
                    
                    if (vehicleAssignment) {
                        const assignmentData = vehicleAssignment.data;
                        
                        const vLat = parseFloat(assignmentData.vLat?.value);
                        const vLng = parseFloat(assignmentData.vLng?.value);
                        const dLat = parseFloat(assignmentData.dLat?.value);
                        const dLng = parseFloat(assignmentData.dLng?.value);
                        const deliveryId = assignmentData.deliveryId?.value;
                        
                        if (vLat && vLng && dLat && dLng && deliveryId) {
                            console.log(`🗺️ Creating route: ${vehicleId} (${vLat}, ${vLng}) → ${deliveryId} (${dLat}, ${dLng})`);
                            
                            // Clear any existing tracked route and delivery marker (but preserve assignment routes)
                            this.clearTrackedRoute();
                            
                            // Create and display the delivery marker for this specific delivery
                            this.addTrackedDeliveryMarker(deliveryId, dLat, dLng);
                            
                            // Create and display the route
                            this.createTrackedRoute(vehicleId, deliveryId, vLat, vLng, dLat, dLng);
                            
                            this.showNotification(`Route displayed for ${vehicleId} → ${deliveryId}`, 'success');
                        } else {
                            console.warn(`⚠️ Missing coordinate data for ${vehicleId}`);
                            this.showNotification(`No route data available for ${vehicleId}`, 'warning');
                        }
                    } else {
                        console.log(`📭 No vehicle assignment found for ${vehicleId}`);
                        this.showNotification(`No active assignment for ${vehicleId}`, 'info');
                    }
                } else {
                    console.log(`📭 No active assignments found`);
                    this.showNotification(`No active assignment for ${vehicleId}`, 'info');
                }
            } else {
                console.error('❌ Failed to fetch vehicle assignment:', response.status);
                this.showNotification('Failed to load route data', 'error');
            }
        } catch (error) {
            console.error('Error displaying vehicle route:', error);
            this.showNotification('Error loading route', 'error');
        }
    }

    clearTrackedRoute() {
        // Clear any existing tracked route - remove layers before sources
        if (this.trackedRoute) {
            if (this.trackedRoute.layer && this.map.getLayer(this.trackedRoute.layer)) {
                this.map.removeLayer(this.trackedRoute.layer);
            }
            if (this.trackedRoute.source && this.map.getSource(this.trackedRoute.source)) {
                this.map.removeSource(this.trackedRoute.source);
            }
            this.trackedRoute = null;
        }
        
        // Clear any existing tracked delivery marker
        if (this.trackedDeliveryMarker) {
            this.trackedDeliveryMarker.remove();
            this.trackedDeliveryMarker = null;
        }
        
        // Exit tracking mode - restore all vehicles
        if (this.trackedVehicleId) {
            this.exitTrackingMode();
        }
    }

    addTrackedDeliveryMarker(deliveryId, lat, lng) {
        if (!this.map) return;
        
        console.log(`📦 Adding tracked delivery marker: ${deliveryId} at (${lat}, ${lng})`);
        
        // Create delivery marker element
        const markerElement = document.createElement('div');
        markerElement.className = 'delivery-marker';
        
        markerElement.innerHTML = `
            <div style="
                width: 28px; 
                height: 28px; 
                background: linear-gradient(135deg, #f59e0b, #d97706);
                border: 3px solid #ffffff;
                border-radius: 8px;
                display: flex;
                align-items: center;
                justify-content: center;
                box-shadow: 0 6px 16px rgba(0,0,0,0.7), 0 0 0 2px rgba(0,0,0,0.3);
                cursor: pointer;
                position: relative;
                font-size: 14px;
                transition: all 0.3s ease;
                backdrop-filter: blur(10px);
            " onmouseover="this.style.transform='scale(1.2)'; this.style.zIndex='1000';" onmouseout="this.style.transform='scale(1)'; this.style.zIndex='auto';">
                <div style="font-size: 12px; line-height: 1; filter: drop-shadow(0 2px 4px rgba(0,0,0,0.7));">📦</div>
            </div>
            <div style="
                position: absolute;
                top: 32px;
                left: 50%;
                transform: translateX(-50%);
                background: linear-gradient(135deg, rgba(245,158,11,0.95), rgba(217,119,6,0.8));
                color: #ffffff;
                padding: 2px 6px;
                border-radius: 4px;
                font-size: 9px;
                font-weight: 800;
                white-space: nowrap;
                box-shadow: 0 4px 12px rgba(0,0,0,0.8);
                border: 1px solid rgba(255,255,255,0.4);
                backdrop-filter: blur(5px);
                text-shadow: 0 1px 2px rgba(0,0,0,0.9);
            ">${deliveryId}</div>
        `;
        
        // Create and add the marker to the map
        this.trackedDeliveryMarker = new tt.Marker({ element: markerElement })
            .setLngLat([lng, lat])
            .addTo(this.map);
        
        // Add popup for delivery marker
        const popup = new tt.Popup({ 
            offset: 25,
            className: 'custom-popup',
            closeOnClick: false,
            closeButton: true
        })
        .setHTML(`
            <div class="p-4">
                <h3 class="font-semibold text-slate-900 mb-2">📦 Delivery Destination</h3>
                <div class="space-y-2">
                    <div><span class="text-slate-600">Delivery ID:</span> <span class="font-medium">${deliveryId}</span></div>
                    <div><span class="text-slate-600">Location:</span> <span class="font-medium">${lat.toFixed(4)}, ${lng.toFixed(4)}</span></div>
                    <div><span class="text-slate-600">Status:</span> <span class="text-amber-600 font-medium">Tracked Destination</span></div>
                </div>
            </div>
        `);
        
        // Add click event to show popup
        markerElement.addEventListener('click', () => {
            popup.setLngLat([lng, lat]).addTo(this.map);
        });
        
        console.log(`✅ Tracked delivery marker added: ${deliveryId}`);
    }

    async createTrackedRoute(vehicleId, deliveryId, vLat, vLng, dLat, dLng) {
        const sourceId = `tracked-route`;
        const layerId = `tracked-route-line`;
        
        console.log(`🛣️ Calculating TomTom route: ${vehicleId} → ${deliveryId}`);
        
        try {
            // Use TomTom route calculation API for actual road-based routes with geometry
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'route'
                },
                body: JSON.stringify({
                    from_lat: vLat,
                    from_lng: vLng,
                    to_lat: dLat,
                    to_lng: dLng,
                    vehicle_id: vehicleId,
                    delivery_id: deliveryId
                })
            });
            
            if (response.ok) {
                const routeData = await response.json();
                console.log(`🗺️ TomTom route response:`, routeData);
                console.log(`🔍 Response keys:`, Object.keys(routeData));
                console.log(`🔍 Response status:`, routeData.status);
                
                let routeGeoJSON;
                
                if (routeData.status === 'success' && routeData.route_geometry && routeData.route_geometry.coordinates) {
                    const coordinates = routeData.route_geometry.coordinates;
                    
                    routeGeoJSON = {
                        type: 'Feature',
                        geometry: {
                            type: 'LineString',
                            coordinates: coordinates
                        },
                        properties: {
                            vehicleId: vehicleId,
                            deliveryId: deliveryId,
                            routeType: 'tomtom',
                            distance: routeData.distance_meters || 0,
                            duration: routeData.duration_seconds || 0
                        }
                    };
                    console.log(`✅ TomTom route calculated: ${coordinates.length} points, ${(routeData.distance_meters / 1000).toFixed(1)}km`);
                } else {
                    console.error('❌ TomTom backend did not return route geometry');
                    console.log('Response structure:', Object.keys(routeData));
                    throw new Error('No route geometry in TomTom response');
                }
                
                // Enter tracking mode and focus on this vehicle
                this.enterTrackingMode(vehicleId);
                
                // Add the TomTom route to map
                if (this.map.isStyleLoaded()) {
                    this.addTrackedRouteToMap(sourceId, layerId, routeGeoJSON, vehicleId, deliveryId);
                } else {
                    this.map.on('styledata', () => {
                        this.addTrackedRouteToMap(sourceId, layerId, routeGeoJSON, vehicleId, deliveryId);
                    });
                }
                
            } else {
                throw new Error(`TomTom route API failed: ${response.status}`);
            }
            
        } catch (error) {
            console.error(`❌ TomTom route calculation failed: ${error.message}`);
            this.showNotification(`Failed to calculate route for ${vehicleId}`, 'error');
        }
    }

    async getTomTomTrafficForRoute(fromLat, fromLng, toLat, toLng) {
        try {
            console.log(`🚦 Checking traffic conditions for route...`);
            
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'traffic'
                },
                body: JSON.stringify({
                    from_lat: fromLat,
                    from_lng: fromLng,
                    to_lat: toLat,
                    to_lng: toLng,
                    include_incidents: true
                })
            });
            
            if (response.ok) {
                const trafficData = await response.json();
                console.log(`🚦 Traffic data received:`, trafficData);
                
                if (trafficData.status === 'success') {
                    return {
                        routeType: trafficData.heavy_traffic ? 'eco' : 'fastest',
                        avoidTraffic: trafficData.heavy_traffic || false,
                        trafficDelay: trafficData.traffic_delay || 0,
                        incidents: trafficData.incidents || []
                    };
                } else {
                    console.warn('⚠️ Traffic API returned error:', trafficData.error);
                    return { routeType: 'fastest', avoidTraffic: false };
                }
            } else {
                console.warn('⚠️ Traffic API request failed:', response.status);
                return { routeType: 'fastest', avoidTraffic: false };
            }
        } catch (error) {
            console.warn('⚠️ Traffic API error:', error.message);
            return { routeType: 'fastest', avoidTraffic: false };
        }
    }

    enterTrackingMode(vehicleId) {
        console.log(`🎯 Entering tracking mode for vehicle: ${vehicleId}`);
        this.trackedVehicleId = vehicleId;
        
        // Dim all other vehicles and highlight the tracked one
        this.vehicleMarkers.forEach((marker, id) => {
            const markerElement = marker.getElement();
            if (id === vehicleId) {
                // Highlight tracked vehicle
                markerElement.style.opacity = '1.0';
                markerElement.style.transform = 'scale(1.2)';
                markerElement.style.zIndex = '1000';
                markerElement.style.filter = 'drop-shadow(0 0 20px rgba(34, 197, 94, 0.8))';
            } else {
                // Dim other vehicles
                markerElement.style.opacity = '0.3';
                markerElement.style.transform = 'scale(0.8)';
                markerElement.style.zIndex = '100';
                markerElement.style.filter = 'grayscale(70%)';
            }
        });
        
        // Keep assignment routes visible - they provide important context
        // Assignment routes remain visible to show all active vehicle assignments
        
        // Update the tracked vehicle's popup to show "Stop Tracking" button
        this.updateVehiclePopupForTracking(vehicleId);
        
        this.showNotification(`🎯 Tracking ${vehicleId} - Click "Stop Tracking" to exit`, 'info');
    }

    exitTrackingMode() {
        if (!this.trackedVehicleId) return;
        
        console.log(`🔄 Exiting tracking mode for vehicle: ${this.trackedVehicleId}`);
        
        // Restore all vehicle markers to normal appearance
        this.vehicleMarkers.forEach((marker, id) => {
            const markerElement = marker.getElement();
            markerElement.style.opacity = '1.0';
            markerElement.style.transform = 'scale(1.0)';
            markerElement.style.zIndex = 'auto';
            markerElement.style.filter = 'none';
        });
        
        // Assignment routes remain visible throughout tracking
        
        // Restore the tracked vehicle's popup to normal
        this.restoreVehiclePopupFromTracking(this.trackedVehicleId);
        
        this.trackedVehicleId = null;
        this.showNotification('🔄 Tracking mode disabled - All vehicles visible', 'info');
    }

    fitMapToRoute(routeGeoJSON, vehicleId, deliveryId) {
        if (!routeGeoJSON || !routeGeoJSON.geometry || !routeGeoJSON.geometry.coordinates) {
            console.warn('⚠️ Cannot fit map to route - no coordinates available');
            return;
        }
        
        const coordinates = routeGeoJSON.geometry.coordinates;
        
        // Calculate bounding box for the route
        let minLng = coordinates[0][0], maxLng = coordinates[0][0];
        let minLat = coordinates[0][1], maxLat = coordinates[0][1];
        
        coordinates.forEach(coord => {
            const [lng, lat] = coord;
            minLng = Math.min(minLng, lng);
            maxLng = Math.max(maxLng, lng);
            minLat = Math.min(minLat, lat);
            maxLat = Math.max(maxLat, lat);
        });
        
        // Add padding to the bounding box
        const padding = 0.1; // degrees
        const bounds = [
            [minLng - padding, minLat - padding], // Southwest
            [maxLng + padding, maxLat + padding]  // Northeast
        ];
        
        console.log(`🗺️ Fitting map to route bounds: ${vehicleId} → ${deliveryId}`);
        
        // Fit the map to show the entire route
        this.map.fitBounds(bounds, {
            padding: { top: 50, bottom: 50, left: 50, right: 50 },
            duration: 2000,
            maxZoom: 10
        });
    }

    updateVehiclePopupForTracking(vehicleId) {
        const actionsContainer = document.getElementById(`vehicle-actions-${vehicleId}`);
        if (actionsContainer) {
            actionsContainer.innerHTML = `
                <button onclick="fleetCommand.clearTrackedRoute()" class="text-xs px-4 py-2 bg-red-500 hover:bg-red-600 border border-red-500 rounded text-white transition-colors">
                    🛑 Stop Tracking
                </button>
                <button onclick="fleetCommand.toggleVehicleDetails('${vehicleId}')" class="text-xs px-4 py-2 bg-gray-100 hover:bg-gray-200 border border-gray-300 rounded text-gray-700 transition-colors">
                    <span id="details-btn-${vehicleId}">Show Details</span>
                </button>
            `;
        }
    }

    restoreVehiclePopupFromTracking(vehicleId) {
        const actionsContainer = document.getElementById(`vehicle-actions-${vehicleId}`);
        if (actionsContainer) {
            actionsContainer.innerHTML = `
                <button onclick="fleetCommand.trackVehicle('${vehicleId}')" class="text-xs px-4 py-2 bg-blue-500 hover:bg-blue-600 border border-blue-500 rounded text-white transition-colors">
                    Track Vehicle
                </button>
                <button onclick="fleetCommand.toggleVehicleDetails('${vehicleId}')" class="text-xs px-4 py-2 bg-gray-100 hover:bg-gray-200 border border-gray-300 rounded text-gray-700 transition-colors">
                    <span id="details-btn-${vehicleId}">Show Details</span>
                </button>
            `;
        }
    }

    addTrackedRouteToMap(sourceId, layerId, routeGeoJSON, vehicleId, deliveryId) {
        try {
            // Check if source already exists and remove it
            if (this.map.getSource(sourceId)) {
                if (this.map.getLayer(layerId)) {
                    this.map.removeLayer(layerId);
                }
                this.map.removeSource(sourceId);
            }
            
            // Add source
            this.map.addSource(sourceId, {
                type: 'geojson',
                data: routeGeoJSON
            });
            
            // Add layer with distinctive styling for tracked route
            this.map.addLayer({
                id: layerId,
                type: 'line',
                source: sourceId,
                layout: {
                    'line-join': 'round',
                    'line-cap': 'round'
                },
                paint: {
                    'line-color': '#3b82f6', // Blue color for tracked route
                    'line-width': 4,
                    'line-opacity': 0.8,
                    'line-dasharray': [2, 2] // Dashed line to distinguish from regular routes
                }
            });
            
            // Add click handler for tracked route
            this.map.on('click', layerId, (e) => {
                const coordinates = e.lngLat;
                new tt.Popup()
                    .setLngLat(coordinates)
                    .setHTML(`
                        <div class="p-3">
                            <h3 class="font-semibold text-slate-900 mb-2">Tracked Route</h3>
                            <div class="space-y-1">
                                <div><span class="text-slate-600">Vehicle:</span> ${vehicleId}</div>
                                <div><span class="text-slate-600">Delivery:</span> ${deliveryId}</div>
                                <div><span class="text-slate-600">Status:</span> <span class="text-blue-600">Tracking Active</span></div>
                            </div>
                        </div>
                    `)
                    .addTo(this.map);
            });
            
            // Change cursor on hover
            this.map.on('mouseenter', layerId, () => {
                this.map.getCanvas().style.cursor = 'pointer';
            });
            
            this.map.on('mouseleave', layerId, () => {
                this.map.getCanvas().style.cursor = '';
            });
            
            // Store tracked route info
            this.trackedRoute = {
                source: sourceId,
                layer: layerId,
                vehicleId: vehicleId,
                deliveryId: deliveryId
            };
            
            // Fit map to show the entire route
            this.fitMapToRoute(routeGeoJSON, vehicleId, deliveryId);
            
            console.log(`✅ Tracked route added to map: ${vehicleId} → ${deliveryId}`);
            
        } catch (error) {
            console.error('Error adding tracked route to map:', error);
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
                        $assignment isa assignment (assigned-vehicle: $vehicle, assigned-employee: $employee, assigned-delivery: $delivery);
                        $vehicle has vehicle-id "${vehicleId}";
                        $assignment has assigned-at $timestamp,
                                   has assignment-status "active";
                        $delivery has delivery-time $deliveryTime;
                        $employee has employee-name $name,
                                   has performance-rating $rating,
                                   has employee-role $role,
                                   has shift-schedule $schedule;
                        $cert_rel isa has-certification (certified-employee: $employee, held-certification: $cert);
                        $cert has certification-name $cert_name;
                        select $employee, $name, $schedule, $rating, $role, $cert_name, $deliveryTime;
                        sort $deliveryTime asc;
                        limit 1;
                    `
                })
            });

            if (!response.ok) {
                return null;
            }

            const data = await response.json();
            
            if (data.ok && data.ok.answers && data.ok.answers.length > 0) {
                const employee = data.ok.answers[0].data;
                
                // Collect all certifications from the results
                const certifications = data.ok.answers
                    .map(answer => answer.data.cert_name?.value)
                    .filter(cert => cert)
                    .join(', ');
                
                return {
                    name: employee.name?.value || 'Unknown Driver',
                    status: employee.schedule?.value || 'unknown',
                    rating: employee.rating?.value || 0,
                    certifications: certifications || 'None',
                    role: employee.role?.value || 'driver'
                };
            }
            
            return null;
        } catch (error) {
            this.showNotification(`Error fetching driver for vehicle ${vehicleId}`, 'error');
            return null;
        }
    }

    async loadDriverDataForPopup(vehicleId) {
        try {
            // Get current driver data for this vehicle
            const driverData = await this.getDriverForVehicle(vehicleId);
            
            // Find the popup elements to update
            const popup = document.querySelector(`#popup-${vehicleId}`);
            if (!popup) return;
            
            if (driverData) {
                // Update driver name
                const driverNameElement = popup.querySelector('.driver-name');
                if (driverNameElement) {
                    driverNameElement.textContent = driverData.name;
                }
                
                // Update driver status
                const driverStatusElement = popup.querySelector('.driver-status');
                if (driverStatusElement) {
                    driverStatusElement.textContent = driverData.status;
                }
                
                // Update driver rating
                const driverRatingElement = popup.querySelector('.driver-rating');
                if (driverRatingElement) {
                    driverRatingElement.textContent = `${driverData.rating}/5 ⭐`;
                }
                
                // Update driver certifications
                const driverCertsElement = popup.querySelector('.driver-certs');
                if (driverCertsElement) {
                    driverCertsElement.textContent = driverData.certifications;
                }
                
                // Update call and message button references
                const callButton = popup.querySelector(`[onclick*="showCallPanel('${vehicleId}'"]`);
                if (callButton) {
                    callButton.setAttribute('onclick', `fleetCommand.showCallPanel('${vehicleId}', '${driverData.name}')`);
                }
                
                const messageButton = popup.querySelector(`[onclick*="showMessagePanel('${vehicleId}'"]`);
                if (messageButton) {
                    messageButton.setAttribute('onclick', `fleetCommand.showMessagePanel('${vehicleId}', '${driverData.name}')`);
                }
                
                // Update call panel driver name
                const callPanelDriverName = popup.querySelector(`#call-panel-${vehicleId} .text-sm.font-medium`);
                if (callPanelDriverName) {
                    callPanelDriverName.textContent = `Calling ${driverData.name}`;
                }
                
                // Update message panel driver name
                const messagePanelDriverName = popup.querySelector(`#message-panel-${vehicleId} .text-sm.font-medium`);
                if (messagePanelDriverName) {
                    messagePanelDriverName.textContent = `Message ${driverData.name}`;
                }
                
            } else {
                // No driver assigned - show placeholder data
                const driverNameElement = popup.querySelector('.driver-name');
                if (driverNameElement) driverNameElement.textContent = 'No Driver Assigned';
                
                const driverStatusElement = popup.querySelector('.driver-status');
                if (driverStatusElement) driverStatusElement.textContent = 'unassigned';
                
                const driverRatingElement = popup.querySelector('.driver-rating');
                if (driverRatingElement) driverRatingElement.textContent = 'N/A';
                
                const driverCertsElement = popup.querySelector('.driver-certs');
                if (driverCertsElement) driverCertsElement.textContent = 'None';
            }
            
        } catch (error) {
            console.error(`Error loading driver data for vehicle ${vehicleId}:`, error);
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
                                 has delivery-status $deliveryStatus;
                        select $routeId, $pickup, $destination, $deliveryTime, $deliveryStatus;
                    `
                })
            });

            if (deliveryResponse.ok) {
                const deliveryData = await deliveryResponse.json();
                if (deliveryData.ok && deliveryData.ok.answers && deliveryData.ok.answers.length > 0) {
                    const delivery = deliveryData.ok.answers[0].data;
                    const routeId = delivery.routeId?.value || 'Unknown Route';
                    const pickup = delivery.pickup?.value || 'Unknown Pickup';
                    const destination = delivery.destination?.value || 'Unknown Destination';
                    const deliveryStatus = delivery.deliveryStatus?.value || 'Unknown';
                    
                    // Update route information in the UI - use actual route ID from delivery data
                    document.getElementById(`route-status-${vehicleId}`).textContent = routeId;
                    document.getElementById(`traffic-status-${vehicleId}`).textContent = `Status: ${deliveryStatus}`;
                    document.getElementById(`eta-${vehicleId}`).textContent = `${pickup} → ${destination}`;
                    
                    // Try to get destination coordinates for TomTom integration if available
                    // For now, show the route information we have from the database
                }
            } else {
                // No delivery assigned - show default status
                document.getElementById(`route-status-${vehicleId}`).textContent = 'No Active Route';
                document.getElementById(`traffic-status-${vehicleId}`).textContent = 'No Assignment';
                document.getElementById(`eta-${vehicleId}`).textContent = 'Awaiting Assignment';
            }
            
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

    async trackVehicle(vehicleId) {
        this.showNotification(`Started tracking vehicle ${vehicleId}`, 'success');
        
        // Center the map on the vehicle and display its route
        this.centerMapOnVehicle(vehicleId);
        await this.displayVehicleRoute(vehicleId);
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
        // Delivery markers now only show when tracking a vehicle
        // this.addDeliveryMarkers();
        this.addAssignmentRoutes();
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
                    query: `match $d isa delivery, has delivery-id "${deliveryId}"; $emp isa employee, has id "${driverId}"; insert (assigned-delivery: $d, assigned-employee: $emp) isa assignment, has assigned-at ${new Date().toISOString()}, has assignment-status "active";`
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
        // Polling disabled - will use dog-realtime when implemented
        console.log('🔄 Real-time polling disabled - awaiting dog-realtime integration');
    }
    
    setupDeliveryDetailsButton() {
        const deliveryDetailsBtn = document.getElementById('delivery-details-btn');
        if (deliveryDetailsBtn) {
            deliveryDetailsBtn.addEventListener('click', () => {
                this.showDeliveryDetailsPanel();
            });
        }
    }
    
    showDeliveryDetailsPanel() {
        // Remove existing panel if any
        const existingPanel = document.getElementById('delivery-details-panel');
        if (existingPanel) {
            existingPanel.remove();
        }
        
        const panel = document.createElement('div');
        panel.id = 'delivery-details-panel';
        panel.className = 'fixed w-96 bg-white rounded-lg shadow-xl border border-gray-200 z-50 max-h-[90vh] overflow-y-auto';
        
        // Position next to the main assignment panel
        const mainPanel = document.getElementById('driver-assignment-panel');
        if (mainPanel) {
            const mainRect = mainPanel.getBoundingClientRect();
            panel.style.left = `${mainRect.right + 16}px`;
            panel.style.top = `${mainRect.top}px`;
        } else {
            // Fallback positioning
            panel.style.right = '4px';
            panel.style.top = '4px';
        }
        
        panel.innerHTML = `
            <div class="p-4 border-b border-gray-100">
                <div class="flex items-center justify-between">
                    <h3 class="text-lg font-semibold text-slate-800">📦 Delivery Details</h3>
                    <button onclick="this.closest('#delivery-details-panel').remove()" class="text-gray-400 hover:text-gray-600 transition-colors">
                        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
                        </svg>
                    </button>
                </div>
            </div>
            
            <div class="p-4 space-y-4">
                <div class="grid grid-cols-2 gap-4">
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Customer Name</label>
                        <input type="text" id="customer-name" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" placeholder="Enter customer name">
                    </div>
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Package Weight (lbs)</label>
                        <input type="number" id="package-weight" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" placeholder="10" min="0.1" step="0.1" value="10">
                    </div>
                </div>
                
                <div>
                    <label class="block text-sm font-medium text-slate-700 mb-2">Estimated Delivery Window</label>
                    <div id="delivery-calculation" class="bg-gray-50 rounded-lg p-3 text-sm">
                        <div class="text-gray-600 mb-2">Calculating delivery window...</div>
                        <div id="delivery-estimate" class="hidden">
                            <div class="flex justify-between items-center mb-2">
                                <span class="text-gray-700">Pickup Date:</span>
                                <span id="pickup-date-display" class="font-medium">--</span>
                            </div>
                            <div class="flex justify-between items-center mb-2">
                                <span class="text-gray-700">Travel Time:</span>
                                <span id="travel-time-display" class="font-medium">--</span>
                            </div>
                            <div class="flex justify-between items-center mb-3 pt-2 border-t border-gray-200">
                                <span class="text-gray-700 font-medium">Estimated Delivery:</span>
                                <span id="estimated-delivery-display" class="font-semibold text-blue-600">--</span>
                            </div>
                        </div>
                    </div>
                    
                    <div class="mt-3">
                        <label class="block text-sm font-medium text-slate-700 mb-2">Custom Delivery Time (Optional)</label>
                        <input type="datetime-local" id="delivery-time" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" placeholder="Override estimated time">
                        <p class="text-xs text-gray-500 mt-1">Leave empty to use estimated delivery time</p>
                    </div>
                </div>
                
                <div class="flex gap-2 pt-4 border-t border-gray-100">
                    <button type="button" onclick="fleetCommand.saveDeliveryDetails()" class="flex-1 bg-green-500 text-white px-4 py-2 text-sm rounded-lg hover:bg-green-600 transition-colors">
                        ✓ Save Details
                    </button>
                </div>
            </div>
        `;
        
        document.body.appendChild(panel);
        
        // Setup handlers for the new panel
        this.setupDeliveryCalculation();
        this.setupSmartDefaults();
        
        // Restore previously saved delivery details if they exist
        this.restoreDeliveryDetails();
    }
    
    setupDeliveryCalculation() {
        // Calculate delivery window when panel opens
        this.calculateDeliveryWindow();
        
        // Listen for changes in pickup date from main panel
        const pickupDateInput = document.getElementById('assignment-schedule');
        if (pickupDateInput) {
            pickupDateInput.addEventListener('change', () => {
                this.calculateDeliveryWindow();
            });
        }
    }
    
    async calculateDeliveryWindow() {
        const pickupDateInput = document.getElementById('assignment-schedule');
        const pickupAddress = document.getElementById('pickup-address');
        const deliveryAddress = document.getElementById('delivery-address');
        
        const pickupDateDisplay = document.getElementById('pickup-date-display');
        const travelTimeDisplay = document.getElementById('travel-time-display');
        const estimatedDeliveryDisplay = document.getElementById('estimated-delivery-display');
        const deliveryEstimate = document.getElementById('delivery-estimate');
        
        if (!pickupDateInput || !pickupAddress || !deliveryAddress) {
            return;
        }
        
        const pickupDate = pickupDateInput.value;
        const pickup = pickupAddress.value;
        const delivery = deliveryAddress.value;
        
        if (pickupDate && pickup && delivery) {
            // Show loading state
            travelTimeDisplay.textContent = 'Calculating...';
            estimatedDeliveryDisplay.textContent = 'Calculating...';
            deliveryEstimate.classList.remove('hidden');
            
            // Calculate estimated travel time using TomTom API
            const estimatedTravelHours = await this.estimateTravelTime(pickup, delivery);
            
            // Calculate delivery time = pickup time + travel time
            const pickupDateTime = new Date(pickupDate);
            const deliveryDateTime = new Date(pickupDateTime.getTime() + estimatedTravelHours * 60 * 60 * 1000);
            
            // Update displays
            pickupDateDisplay.textContent = pickupDateTime.toLocaleString();
            travelTimeDisplay.textContent = `~${estimatedTravelHours} hours`;
            estimatedDeliveryDisplay.textContent = deliveryDateTime.toLocaleString();
            
            // Set the hidden delivery time input to the calculated time
            const deliveryTimeInput = document.getElementById('delivery-time');
            if (deliveryTimeInput && !deliveryTimeInput.value) {
                deliveryTimeInput.value = deliveryDateTime.toISOString().slice(0, 16);
            }
        }
    }
    
    async estimateTravelTime(pickupAddress, deliveryAddress) {
        try {
            // Get coordinates for both addresses using TomTom geocoding
            const pickupCoords = await this.geocodeAddress(pickupAddress);
            const deliveryCoords = await this.geocodeAddress(deliveryAddress);
            
            if (pickupCoords && deliveryCoords) {
                // Use TomTom routing API for accurate travel time
                const travelTimeHours = await this.getTomTomTravelTime(pickupCoords, deliveryCoords);
                if (travelTimeHours) {
                    return travelTimeHours;
                }
            }
        } catch (error) {
            console.warn('TomTom routing failed, using fallback estimation:', error);
        }
        
        // Fallback to simple estimation if TomTom fails
        const pickup = pickupAddress.toLowerCase();
        const delivery = deliveryAddress.toLowerCase();
        
        if (this.isSameCity(pickup, delivery)) {
            return 1; // 1 hour for local delivery
        }
        
        if (this.isSameState(pickup, delivery)) {
            return 3; // 3 hours for regional delivery
        }
        
        return 8; // 8 hours for long-distance delivery
    }
    
    async getTomTomTravelTime(fromCoords, toCoords) {
        try {
            // Get current assignment context for delivery_id and vehicle_id
            const routeId = document.getElementById('route-id')?.value || 'temp-route';
            const vehicleId = document.getElementById('vehicle-assignment')?.value || 'unknown-vehicle';
            
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'route'
                },
                body: JSON.stringify({
                    delivery_id: routeId,
                    vehicle_id: vehicleId,
                    service: 'routing',
                    from_lat: fromCoords.lat,
                    from_lng: fromCoords.lng,
                    to_lat: toCoords.lat,
                    to_lng: toCoords.lng
                })
            });
            
            const data = await response.json();
            
            if (data.status === 'success' && data.duration_seconds) {
                const travelTimeHours = Math.ceil(data.duration_seconds / 3600); // Convert seconds to hours, round up
                console.log(`TomTom routing: ${travelTimeHours} hours (${data.duration_seconds}s) for ${fromCoords.lat},${fromCoords.lng} to ${toCoords.lat},${toCoords.lng}`);
                return travelTimeHours;
            }
        } catch (error) {
            console.error('TomTom routing API error:', error);
        }
        
        return null;
    }
    
    async geocodeAddress(address) {
        try {
            // Get current assignment context for delivery_id (same as routing call)
            const routeId = document.getElementById('route-id')?.value || 'temp-route';
            
            // Use TomTom backend service for geocoding (same pattern as reverse geocoding)
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'geocode'
                },
                body: JSON.stringify({
                    address: address,
                    delivery_id: routeId
                })
            });
            
            if (response.ok) {
                const data = await response.json();
                console.log('TomTom geocode response:', data);
                
                if (data.status === 'success') {
                    return { lat: data.latitude, lng: data.longitude };
                } else {
                    throw new Error('Invalid geocoding response');
                }
            } else {
                throw new Error(`Geocoding failed: ${response.status}`);
            }
        } catch (error) {
            console.error('Geocoding error:', error);
        }
        
        return null;
    }
    
    restoreDeliveryDetails() {
        // Restore previously saved delivery details if they exist
        if (this.deliveryDetails) {
            const customerNameInput = document.getElementById('customer-name');
            const packageWeightInput = document.getElementById('package-weight');
            const deliveryTimeInput = document.getElementById('delivery-time');
            
            if (customerNameInput && this.deliveryDetails.customerName) {
                customerNameInput.value = this.deliveryDetails.customerName;
            }
            
            if (packageWeightInput && this.deliveryDetails.packageWeight) {
                packageWeightInput.value = this.deliveryDetails.packageWeight;
            }
            
            if (deliveryTimeInput && this.deliveryDetails.deliveryTime) {
                deliveryTimeInput.value = this.deliveryDetails.deliveryTime;
            }
            
            console.log('Restored delivery details:', this.deliveryDetails);
        }
    }
    
    saveDeliveryDetails() {
        try {
            // Get all the delivery detail values
            const customerName = document.getElementById('customer-name')?.value || '';
            const packageWeight = document.getElementById('package-weight')?.value || '';
            const deliveryTime = document.getElementById('delivery-time')?.value || '';
            
            // Get delivery priority from main panel (not duplicate priority level)
            const deliveryPriority = document.getElementById('delivery-priority')?.value || 'standard';
            
            // Store the values persistently so they're available when panel is reopened
            this.deliveryDetails = {
                customerName,
                packageWeight,
                deliveryTime,
                deliveryPriority
            };
            
            // Close the delivery details panel
            const panel = document.getElementById('delivery-details-panel');
            if (panel) {
                panel.remove();
            }
            
            // Show success feedback
            console.log('Delivery details saved:', this.deliveryDetails);
            
            // Optional: Show a brief success message
            this.showNotification('Delivery details saved successfully', 'success');
            
        } catch (error) {
            console.error('Error saving delivery details:', error);
            this.showNotification('Error saving delivery details', 'error');
        }
    }
    
    isSameCity(pickup, delivery) {
        // Extract city names and compare
        const pickupParts = pickup.split(',');
        const deliveryParts = delivery.split(',');
        
        if (pickupParts.length > 1 && deliveryParts.length > 1) {
            const pickupCity = pickupParts[1].trim();
            const deliveryCity = deliveryParts[1].trim();
            return pickupCity === deliveryCity;
        }
        
        return false;
    }
    
    isSameState(pickup, delivery) {
        // Extract state/region and compare
        const pickupParts = pickup.split(',');
        const deliveryParts = delivery.split(',');
        
        if (pickupParts.length > 2 && deliveryParts.length > 2) {
            const pickupState = pickupParts[2].trim().substring(0, 2);
            const deliveryState = deliveryParts[2].trim().substring(0, 2);
            return pickupState === deliveryState;
        }
        
        return false;
    }
    
    setupSmartDefaults() {
        const customerNameInput = document.getElementById('customer-name');
        const deliveryAddressInput = document.getElementById('delivery-address');
        const packageWeightInput = document.getElementById('package-weight');
        const priorityLevelSelect = document.getElementById('priority-level');
        const deliveryPrioritySelect = document.getElementById('delivery-priority');
        
        // Auto-generate customer name from delivery address
        if (deliveryAddressInput && customerNameInput) {
            deliveryAddressInput.addEventListener('blur', () => {
                if (!customerNameInput.value && deliveryAddressInput.value) {
                    const address = deliveryAddressInput.value;
                    const parts = address.split(',');
                    if (parts.length > 0) {
                        const streetOrBuilding = parts[0].trim();
                        customerNameInput.value = `Customer at ${streetOrBuilding}`;
                    }
                }
            });
        }
        
        // Sync priority levels
        if (deliveryPrioritySelect && priorityLevelSelect) {
            deliveryPrioritySelect.addEventListener('change', () => {
                const priorityMap = {
                    'low': '1',
                    'standard': '1', 
                    'high': '2',
                    'urgent': '4',
                    'critical': '5'
                };
                
                const numericPriority = priorityMap[deliveryPrioritySelect.value] || '1';
                priorityLevelSelect.value = numericPriority;
            });
        }
        
        // Adjust weight based on priority
        if (priorityLevelSelect && packageWeightInput) {
            priorityLevelSelect.addEventListener('change', () => {
                const currentWeight = parseFloat(packageWeightInput.value) || 10;
                if (currentWeight === 10) { // Only adjust if still default
                    const weightMap = {
                        '1': 10,
                        '2': 15,
                        '3': 20,
                        '4': 25,
                        '5': 30
                    };
                    packageWeightInput.value = weightMap[priorityLevelSelect.value] || 10;
                }
            });
        }
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
                query: `match $d isa delivery, has delivery-id "${deliveryId}"; $emp isa employee, has id "${driverId}"; insert (assigned-delivery: $d, assigned-employee: $emp) isa assignment, has assigned-at ${new Date().toISOString()}, has assignment-status "active";`
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
    // Polling disabled - will use dog-realtime when implemented
    console.log('🔄 Real-time polling disabled - awaiting dog-realtime integration');
}

    updateDriversView() {
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
                        <label class="block text-sm font-medium text-slate-700 mb-2">Vehicle Pickup Date</label>
                        <input type="datetime-local" id="assignment-schedule" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                    </div>
                    
                    <div>
                        <label class="block text-sm font-medium text-slate-700 mb-2">Vehicle Assignment</label>
                        <select id="vehicle-assignment" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                            <option value="">Select a vehicle...</option>
                        </select>
                    </div>
                    
                    <div id="pickup-location-container" class="hidden">
                        <label class="block text-sm font-medium text-slate-700 mb-2">Pickup Location</label>
                        <div class="relative">
                            <input type="text" id="pickup-address" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" placeholder="Auto-populated from vehicle location" readonly>
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
                            <option value="low">Low</option>
                            <option value="standard" selected>Standard</option>
                            <option value="high">High</option>
                            <option value="urgent">Urgent</option>
                            <option value="critical">Critical</option>
                        </select>
                    </div>
                    
                    <div class="flex gap-2">
                        <button type="button" id="delivery-details-btn" class="flex-1 bg-blue-500 text-white px-4 py-2 text-sm rounded-lg hover:bg-blue-600 transition-colors">
                            📦 Delivery Details
                        </button>
                    </div>
                    
                    <div id="certification-requirements-container" class="hidden">
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
                    
                    <div id="driver-assignment-container" class="hidden">
                        <label class="block text-sm font-medium text-slate-700 mb-2">Driver Assignment</label>
                        <select id="driver-assignment" class="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                            <option value="">Select a driver...</option>
                        </select>
                    </div>
                    
                    <div class="flex gap-3 pt-4">
                        <button type="submit" id="assignment-submit-btn" class="flex-1 bg-gray-400 text-white px-4 py-2 text-sm rounded-lg cursor-not-allowed transition-colors" disabled>
                            <span id="assignment-btn-text">Create Assignment</span>
                        </button>
                        <button type="button" onclick="fleetCommand.hideDriverAssignmentPanel()" class="px-4 py-2 text-sm border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors">
                            Cancel
                        </button>
                    </div>
                </form>
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
            this.showAssignmentConfirmationPopup();
        });
        
        // Setup address autocomplete
        this.setupAddressAutocomplete();
        
        // Setup datetime constraints
        this.setupDateTimeConstraints();
        
        // Load available vehicles and setup date-dependent driver loading
        setTimeout(() => {
            this.setupScheduleDependentFormFlow();
            this.setupDateDependentDriverLoading();
            this.setupDeliveryDetailsButton();
            this.setupFormValidation();
        }, 500);
        
        return panel;
    }
    
    showAssignmentConfirmationPopup() {
        // Validate form first
        const routeId = document.getElementById('route-id').value;
        const vehicleId = document.getElementById('vehicle-assignment').value;
        const driverId = document.getElementById('driver-assignment').value;
        const pickupLat = parseFloat(document.getElementById('pickup-lat').value);
        const pickupLng = parseFloat(document.getElementById('pickup-lng').value);
        
        if (!routeId || !vehicleId || !driverId || isNaN(pickupLat) || isNaN(pickupLng)) {
            this.showNotification('Please fill in all required fields including route ID, vehicle, driver, and pickup address', 'error');
            return;
        }
        
        // Create confirmation modal
        const modal = document.createElement('div');
        modal.id = 'assignment-confirmation-modal';
        modal.className = 'fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50';
        
        modal.innerHTML = `
            <div class="bg-white rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
                <div class="flex items-center justify-between mb-4">
                    <h3 class="text-lg font-semibold text-slate-900">Confirm Assignment</h3>
                    <button onclick="fleetCommand.closeAssignmentConfirmationPopup()" class="text-slate-400 hover:text-slate-600">
                        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>
                        </svg>
                    </button>
                </div>
                
                <div class="mb-6">
                    <p class="text-slate-600 mb-4">How would you like to process this assignment?</p>
                    
                    <div class="space-y-3">
                        <button onclick="fleetCommand.confirmAssignment('immediate')" class="w-full flex items-center justify-between p-4 border border-blue-200 rounded-lg hover:bg-blue-50 transition-colors">
                            <div class="text-left">
                                <div class="font-medium text-slate-900">Confirm Now</div>
                                <div class="text-sm text-slate-500">Start assignment immediately</div>
                            </div>
                            <svg class="w-5 h-5 text-blue-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 7l5 5m0 0l-5 5m5-5H6"/>
                            </svg>
                        </button>
                        
                        <button onclick="fleetCommand.confirmAssignment('scheduled')" class="w-full flex items-center justify-between p-4 border border-gray-200 rounded-lg hover:bg-gray-50 transition-colors">
                            <div class="text-left">
                                <div class="font-medium text-slate-900">Schedule for Later</div>
                                <div class="text-sm text-slate-500">Start at scheduled time</div>
                            </div>
                            <svg class="w-5 h-5 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"/>
                            </svg>
                        </button>
                    </div>
                </div>
                
                <div class="flex justify-end">
                    <button onclick="fleetCommand.closeAssignmentConfirmationPopup()" class="px-4 py-2 text-sm border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors">
                        Cancel
                    </button>
                </div>
            </div>
        `;
        
        document.body.appendChild(modal);
    }
    
    closeAssignmentConfirmationPopup() {
        const modal = document.getElementById('assignment-confirmation-modal');
        if (modal) {
            modal.remove();
        }
    }
    
    confirmAssignment(timing) {
        // Close the confirmation popup
        this.closeAssignmentConfirmationPopup();
        
        if (timing === 'scheduled') {
            // Show scheduling UI for later assignments
            this.showSchedulingInterface();
        } else {
            // Submit assignment immediately
            this.submitDriverAssignment(timing);
        }
    }
    
    showSchedulingInterface() {
        // Create scheduling modal
        const modal = document.createElement('div');
        modal.id = 'scheduling-interface-modal';
        modal.className = 'fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50';
        
        // Get current date/time for default values
        const now = new Date();
        const tomorrow = new Date(now);
        tomorrow.setDate(tomorrow.getDate() + 1);
        tomorrow.setHours(9, 0, 0, 0); // Default to 9 AM tomorrow
        
        const defaultDate = tomorrow.toISOString().split('T')[0];
        const defaultTime = '09:00';
        
        modal.innerHTML = `
            <div class="bg-white rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
                <div class="flex items-center justify-between mb-4">
                    <h3 class="text-lg font-semibold text-slate-900">Schedule Assignment</h3>
                    <button onclick="fleetCommand.closeSchedulingInterface()" class="text-slate-400 hover:text-slate-600">
                        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/>
                        </svg>
                    </button>
                </div>
                
                <div class="mb-6">
                    <p class="text-slate-600 mb-4">When would you like this assignment to start?</p>
                    
                    <div class="space-y-4">
                        <div class="grid grid-cols-2 gap-4">
                            <div>
                                <label class="block text-sm font-medium text-slate-700 mb-2">Date</label>
                                <input type="date" id="scheduled-date" class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" value="${defaultDate}" min="${now.toISOString().split('T')[0]}">
                            </div>
                            
                            <div>
                                <label class="block text-sm font-medium text-slate-700 mb-2">Time</label>
                                <input type="time" id="scheduled-time" class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" value="${defaultTime}">
                            </div>
                        </div>
                        
                        <div>
                            <label class="block text-sm font-medium text-slate-700 mb-2">Repeat Schedule</label>
                            <select id="repeat-frequency" class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                                <option value="none">No repeat (One-time)</option>
                                <option value="daily">Daily</option>
                                <option value="weekly">Weekly</option>
                                <option value="monthly">Monthly</option>
                                <option value="custom">Custom interval</option>
                            </select>
                        </div>
                        
                        <div id="custom-interval-container" class="hidden">
                            <label class="block text-sm font-medium text-slate-700 mb-2">Custom Interval</label>
                            <div class="flex gap-2">
                                <input type="number" id="custom-interval-value" class="flex-1 px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" min="1" value="1" placeholder="1">
                                <select id="custom-interval-unit" class="px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                                    <option value="days">Days</option>
                                    <option value="weeks">Weeks</option>
                                    <option value="months">Months</option>
                                </select>
                            </div>
                        </div>
                        
                        <div id="repeat-options-container" class="hidden">
                            <div class="border border-gray-200 rounded-lg p-3 space-y-3">
                                <h4 class="text-sm font-medium text-slate-700">Repeat Options</h4>
                                
                                <div>
                                    <label class="block text-sm font-medium text-slate-700 mb-2">End Condition</label>
                                    <select id="repeat-end-condition" class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent">
                                        <option value="never">Never end</option>
                                        <option value="after-occurrences">After number of occurrences</option>
                                        <option value="end-date">End by date</option>
                                    </select>
                                </div>
                                
                                <div id="occurrences-container" class="hidden">
                                    <label class="block text-sm font-medium text-slate-700 mb-2">Number of Occurrences</label>
                                    <input type="number" id="max-occurrences" class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" min="1" value="10" placeholder="10">
                                </div>
                                
                                <div id="end-date-container" class="hidden">
                                    <label class="block text-sm font-medium text-slate-700 mb-2">End Date</label>
                                    <input type="date" id="repeat-end-date" class="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent" min="${now.toISOString().split('T')[0]}">
                                </div>
                                
                                <div class="flex items-center">
                                    <input type="checkbox" id="skip-weekends" class="mr-2">
                                    <label for="skip-weekends" class="text-sm text-slate-700">Skip weekends (Saturday & Sunday)</label>
                                </div>
                                
                                <div class="flex items-center">
                                    <input type="checkbox" id="skip-holidays" class="mr-2">
                                    <label for="skip-holidays" class="text-sm text-slate-700">Skip holidays</label>
                                </div>
                            </div>
                        </div>
                        
                        <div class="bg-blue-50 border border-blue-200 rounded-lg p-3">
                            <div class="flex items-start">
                                <svg class="w-5 h-5 text-blue-500 mt-0.5 mr-2" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
                                </svg>
                                <div class="text-sm text-blue-700">
                                    <p class="font-medium">Scheduling Note</p>
                                    <p id="scheduling-summary">The assignment will be queued and automatically activated at the scheduled time.</p>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
                
                <div class="flex gap-3">
                    <button onclick="fleetCommand.scheduleAssignment()" class="flex-1 bg-blue-600 text-white px-4 py-2 text-sm rounded-lg hover:bg-blue-700 transition-colors">
                        Schedule Assignment
                    </button>
                    <button onclick="fleetCommand.closeSchedulingInterface()" class="px-4 py-2 text-sm border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors">
                        Cancel
                    </button>
                </div>
            </div>
        `;
        
        document.body.appendChild(modal);
        
        // Add event listeners for dynamic UI updates
        this.setupSchedulingEventHandlers();
    }
    
    setupSchedulingEventHandlers() {
        const repeatFrequency = document.getElementById('repeat-frequency');
        const customIntervalContainer = document.getElementById('custom-interval-container');
        const repeatOptionsContainer = document.getElementById('repeat-options-container');
        const repeatEndCondition = document.getElementById('repeat-end-condition');
        const occurrencesContainer = document.getElementById('occurrences-container');
        const endDateContainer = document.getElementById('end-date-container');
        const schedulingSummary = document.getElementById('scheduling-summary');
        
        // Handle repeat frequency changes
        repeatFrequency.addEventListener('change', () => {
            const frequency = repeatFrequency.value;
            
            if (frequency === 'none') {
                customIntervalContainer.classList.add('hidden');
                repeatOptionsContainer.classList.add('hidden');
                schedulingSummary.textContent = 'The assignment will be queued and automatically activated at the scheduled time.';
            } else {
                repeatOptionsContainer.classList.remove('hidden');
                
                if (frequency === 'custom') {
                    customIntervalContainer.classList.remove('hidden');
                } else {
                    customIntervalContainer.classList.add('hidden');
                }
                
                this.updateSchedulingSummary();
            }
        });
        
        // Handle end condition changes
        repeatEndCondition.addEventListener('change', () => {
            const condition = repeatEndCondition.value;
            
            occurrencesContainer.classList.add('hidden');
            endDateContainer.classList.add('hidden');
            
            if (condition === 'after-occurrences') {
                occurrencesContainer.classList.remove('hidden');
            } else if (condition === 'end-date') {
                endDateContainer.classList.remove('hidden');
            }
            
            this.updateSchedulingSummary();
        });
        
        // Handle other field changes for summary updates
        document.getElementById('custom-interval-value').addEventListener('input', () => this.updateSchedulingSummary());
        document.getElementById('custom-interval-unit').addEventListener('change', () => this.updateSchedulingSummary());
        document.getElementById('max-occurrences').addEventListener('input', () => this.updateSchedulingSummary());
        document.getElementById('repeat-end-date').addEventListener('change', () => this.updateSchedulingSummary());
        document.getElementById('skip-weekends').addEventListener('change', () => this.updateSchedulingSummary());
        document.getElementById('skip-holidays').addEventListener('change', () => this.updateSchedulingSummary());
    }
    
    updateSchedulingSummary() {
        const frequency = document.getElementById('repeat-frequency').value;
        const endCondition = document.getElementById('repeat-end-condition').value;
        const schedulingSummary = document.getElementById('scheduling-summary');
        
        if (frequency === 'none') {
            schedulingSummary.textContent = 'The assignment will be queued and automatically activated at the scheduled time.';
            return;
        }
        
        let summary = 'This assignment will repeat ';
        
        // Add frequency description
        if (frequency === 'daily') {
            summary += 'daily';
        } else if (frequency === 'weekly') {
            summary += 'weekly';
        } else if (frequency === 'monthly') {
            summary += 'monthly';
        } else if (frequency === 'custom') {
            const value = document.getElementById('custom-interval-value').value || '1';
            const unit = document.getElementById('custom-interval-unit').value;
            summary += `every ${value} ${unit}`;
        }
        
        // Add end condition
        if (endCondition === 'never') {
            summary += ' indefinitely';
        } else if (endCondition === 'after-occurrences') {
            const occurrences = document.getElementById('max-occurrences').value || '10';
            summary += ` for ${occurrences} occurrences`;
        } else if (endCondition === 'end-date') {
            const endDate = document.getElementById('repeat-end-date').value;
            if (endDate) {
                summary += ` until ${new Date(endDate).toLocaleDateString()}`;
            }
        }
        
        // Add skip options
        const skipWeekends = document.getElementById('skip-weekends').checked;
        const skipHolidays = document.getElementById('skip-holidays').checked;
        
        if (skipWeekends || skipHolidays) {
            summary += ', skipping';
            if (skipWeekends) summary += ' weekends';
            if (skipWeekends && skipHolidays) summary += ' and';
            if (skipHolidays) summary += ' holidays';
        }
        
        summary += '.';
        schedulingSummary.textContent = summary;
    }
    
    closeSchedulingInterface() {
        const modal = document.getElementById('scheduling-interface-modal');
        if (modal) {
            modal.remove();
        }
    }
    
    scheduleAssignment() {
        const scheduledDate = document.getElementById('scheduled-date').value;
        const scheduledTime = document.getElementById('scheduled-time').value;
        
        if (!scheduledDate || !scheduledTime) {
            this.showNotification('Please select both date and time for scheduling', 'error');
            return;
        }
        
        // Combine date and time into ISO string
        const scheduledDateTime = `${scheduledDate}T${scheduledTime}:00`;
        
        // Validate that scheduled time is in the future
        const scheduledTimestamp = new Date(scheduledDateTime);
        const now = new Date();
        
        if (scheduledTimestamp <= now) {
            this.showNotification('Scheduled time must be in the future', 'error');
            return;
        }
        
        // Collect recurring schedule data
        const repeatFrequency = document.getElementById('repeat-frequency').value;
        const recurringSchedule = {
            frequency: repeatFrequency,
            start_datetime: scheduledDateTime
        };
        
        if (repeatFrequency !== 'none') {
            // Add custom interval data if applicable
            if (repeatFrequency === 'custom') {
                recurringSchedule.custom_interval = {
                    value: parseInt(document.getElementById('custom-interval-value').value) || 1,
                    unit: document.getElementById('custom-interval-unit').value
                };
            }
            
            // Add end condition data
            const endCondition = document.getElementById('repeat-end-condition').value;
            recurringSchedule.end_condition = {
                type: endCondition
            };
            
            if (endCondition === 'after-occurrences') {
                recurringSchedule.end_condition.max_occurrences = parseInt(document.getElementById('max-occurrences').value) || 10;
            } else if (endCondition === 'end-date') {
                const endDate = document.getElementById('repeat-end-date').value;
                if (endDate) {
                    recurringSchedule.end_condition.end_date = endDate;
                }
            }
            
            // Add skip options
            recurringSchedule.skip_options = {
                skip_weekends: document.getElementById('skip-weekends').checked,
                skip_holidays: document.getElementById('skip-holidays').checked
            };
        }
        
        // Close scheduling interface
        this.closeSchedulingInterface();
        
        // Submit assignment with scheduled timing and recurring data
        this.submitDriverAssignment('scheduled', scheduledDateTime, recurringSchedule);
    }
    
    async submitDriverAssignment(assignmentTiming = 'immediate', scheduledDateTime = null, recurringSchedule = null) {
        const routeId = document.getElementById('route-id').value;
        const vehicleId = document.getElementById('vehicle-assignment').value;
        const driverId = document.getElementById('driver-assignment').value;
        const pickupLat = parseFloat(document.getElementById('pickup-lat').value);
        const pickupLng = parseFloat(document.getElementById('pickup-lng').value);
        const deliveryPriority = document.getElementById('delivery-priority').value;
        const scheduleTime = document.getElementById('assignment-schedule').value;
        
        // Use the timing parameter passed from confirmation popup
        
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
        
        // Determine assignment status based on timing selection
        const assignmentStatus = assignmentTiming === 'immediate' ? 'active' : 'scheduled';
        
        // Use the specific scheduled datetime if provided, otherwise fall back to form schedule time
        const finalScheduledTime = scheduledDateTime || scheduleTime || null;
        
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
            scheduled_time: finalScheduledTime,
            assignment_timing: assignmentTiming,
            assignment_status: assignmentStatus,
            recurring_schedule: recurringSchedule
        };
        
        // Helper function to escape TypeQL string literals
        const escapeTypeQLString = (str) => {
            return str.replace(/"/g, '\\"').replace(/'/g, "\\'");
        };

        // Create TypeDB assignment query with proper single 3-role assignment and put for idempotency
        const query = `
            match
            $vehicle isa vehicle, has vehicle-id "${assignmentData.vehicle_id}";
            $driver isa employee, has id "${assignmentData.driver_id}";
            
            put
            $delivery isa delivery,
                has delivery-id "${assignmentData.route_id}",
                has route-id "${assignmentData.route_id}",
                has pickup-address "${escapeTypeQLString(assignmentData.pickup_address)}",
                has delivery-address "${escapeTypeQLString(assignmentData.delivery_address)}",
                has dest-lat ${assignmentData.delivery_location[0]},
                has dest-lng ${assignmentData.delivery_location[1]},
                has customer-name "${escapeTypeQLString(this.deliveryDetails?.customerName || 'Customer ' + assignmentData.route_id)}",
                has delivery-time ${this.deliveryDetails?.deliveryTime ? new Date(this.deliveryDetails.deliveryTime).toISOString().replace(/\.\d{3}Z$/, '') : new Date(Date.now() + 2*60*60*1000).toISOString().replace(/\.\d{3}Z$/, '')},
                has weight ${parseFloat(this.deliveryDetails?.packageWeight) || 10.0},
                has priority 1,
                has customer-priority "${assignmentData.delivery_priority}",
                has delivery-status "pending",
                has created-at ${new Date().toISOString().replace(/\.\d{3}Z$/, '')};
            
            insert
            (assigned-employee: $driver, assigned-vehicle: $vehicle, assigned-delivery: $delivery) isa assignment,
                has assignment-status "${assignmentData.assignment_status}",
                has assigned-at ${new Date().toISOString().replace(/\.\d{3}Z$/, '')};
        `;

        try {
            const response = await fetch(`${this.apiBaseUrl}/operations`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'write'
                },
                body: JSON.stringify({ query: query })
            });
            
            if (response.ok) {
                const result = await response.json();
                this.showNotification('Assignment scheduled successfully', 'success');
                this.clearAssignmentForm();
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
    
    setupScheduleDependentFormFlow() {
        const scheduleInput = document.getElementById('assignment-schedule');
        const vehicleContainer = document.getElementById('vehicle-assignment').parentElement;
        const pickupContainer = document.getElementById('pickup-location-container');
        
        if (!scheduleInput) return;
        
        // Initially hide vehicle and pickup containers
        vehicleContainer.classList.add('hidden');
        pickupContainer.classList.add('hidden');
        
        // Add event listener for schedule changes
        scheduleInput.addEventListener('change', () => {
            const hasSchedule = scheduleInput.value;
            
            if (hasSchedule) {
                // Show vehicle selection and load vehicles with location prediction
                vehicleContainer.classList.remove('hidden');
                this.loadAvailableVehicles();
                
                // Keep pickup location hidden until vehicle is selected
                pickupContainer.classList.add('hidden');
            } else {
                // Hide all containers if no schedule
                vehicleContainer.classList.add('hidden');
                pickupContainer.classList.add('hidden');
                
                // Clear vehicle and pickup selections
                document.getElementById('vehicle-assignment').value = '';
                document.getElementById('pickup-address').value = '';
                document.getElementById('pickup-lat').value = '';
                document.getElementById('pickup-lng').value = '';
            }
        });
    }
    
    setupAssignmentTimingHandlers() {
        const timingRadios = document.querySelectorAll('input[name="assignment-timing"]');
        const buttonText = document.getElementById('assignment-btn-text');
        
        if (!buttonText) return;
        
        // Add event listeners to timing radio buttons
        timingRadios.forEach(radio => {
            radio.addEventListener('change', (event) => {
                const selectedTiming = event.target.value;
                
                if (selectedTiming === 'immediate') {
                    buttonText.textContent = 'Assign Immediately';
                } else if (selectedTiming === 'scheduled') {
                    buttonText.textContent = 'Schedule Assignment';
                }
            });
        });
    }
    
    async loadAvailableVehicles() {
        try {
            const vehicleSelect = document.getElementById('vehicle-assignment');
            const scheduleInput = document.getElementById('assignment-schedule');
            
            if (!vehicleSelect) {
                console.error('Vehicle select element not found');
                return;
            }
            
            // Clear existing options except the first one
            vehicleSelect.innerHTML = '<option value="">Select a vehicle...</option>';
            
            // Get pickup time for location prediction
            const assignmentDateTime = scheduleInput?.value;
            if (!assignmentDateTime) {
                console.log('No assignment time selected, using simple vehicle query');
                // Fallback to simple query if no time selected
                const response = await fetch(`${this.apiBaseUrl}/vehicles`, {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                        'x-service-method': 'read'
                    },
                    body: JSON.stringify({
                        query: 'match $v isa vehicle, has vehicle-id $id, has vehicle-type $type, has vehicle-status $status, has maintenance-status "good", has fuel-level $fuel; $fuel >= 50.0; not { $assignment isa assignment (assigned-vehicle: $v, assigned-employee: $employee); }; select $v, $id, $type, $status; limit 10;'
                    })
                });
                this.processVehicleResponse(response, false);
                return;
            }
            
            // Convert datetime-local format to ISO format for query
            const assignmentDate = new Date(assignmentDateTime);
            const isoDateTime = assignmentDate.toISOString();
            
            // Enhanced Vehicle Selection Query: Find vehicles available at pickup time and predict where they'll be
            // 
            // Query Logic:
            // 1. Filter by maintenance status and fuel level (operational vehicles only)
            // 2. Predict vehicle location at pickup time using disjunction:
            //    - Case A: Vehicle idle → use current GPS position (gps-latitude, gps-longitude)
            //    - Case B: Vehicle busy → use delivery destination (dest-lat, dest-lng)
            // 3. Return vehicles with predicted locations for pickup location auto-population
            const response = await fetch(`${this.apiBaseUrl}/vehicles`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `match $v isa vehicle, has vehicle-id $id, has vehicle-type $type, has vehicle-status $status, has maintenance-status "good", has fuel-level $fuel; $fuel >= 50.0; { $v has gps-latitude $lat, has gps-longitude $lng; not { $assignment1 isa assignment (assigned-vehicle: $v, assigned-delivery: $delivery1), has assigned-at $assignTime1; $assignTime1 == "${isoDateTime}"; }; } or { $assignment2 isa assignment (assigned-vehicle: $v, assigned-delivery: $delivery2), has assigned-at $assignTime2; $assignTime2 == "${isoDateTime}"; $delivery2 has dest-lat $lat, has dest-lng $lng; }; select $v, $id, $type, $status, $lat, $lng; limit 10;`
                })
            });
            
            if (!response.ok) {
                throw new Error(`Failed to fetch available vehicles: ${response.status} ${response.statusText}`);
            }
            
            const result = await response.json();
            console.log('Loaded available vehicles from database:', result);
            
            this.processVehicleResponse(result, true);
            
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
    
    processVehicleResponse(data, hasLocationData) {
        try {
            console.log('Loaded available vehicles from database:', data);
            
            const availableVehicles = data.ok?.answers || [];
            const vehicleSelect = document.getElementById('vehicle-assignment');
            
            // Store vehicle location data for pickup auto-population
            this.vehicleLocations = {};
            
            availableVehicles.forEach(answer => {
                const vehicleData = answer.data;
                if (vehicleData && vehicleData.id) {
                    const vehicleId = vehicleData.id.value;
                    const option = document.createElement('option');
                    option.value = vehicleId;
                    
                    // Store location data if available
                    if (hasLocationData && vehicleData.lat && vehicleData.lng) {
                        this.vehicleLocations[vehicleId] = {
                            lat: vehicleData.lat.value,
                            lng: vehicleData.lng.value
                        };
                        option.textContent = `${vehicleId} - ${vehicleData.type?.value || 'Vehicle'}`;
                    } else {
                        option.textContent = `${vehicleId} - ${vehicleData.type?.value || 'Vehicle'} (${vehicleData.status?.value || 'available'})`;
                    }
                    
                    vehicleSelect.appendChild(option);
                }
            });
            
            console.log(`Loaded ${availableVehicles.length} available vehicles from database`);
            
            // Add event listener for vehicle selection to auto-populate pickup location
            if (hasLocationData) {
                this.setupVehicleLocationAutoPopulation();
            }
            
        } catch (error) {
            console.error('Error processing vehicle response:', error);
        }
    }
    
    setupVehicleLocationAutoPopulation() {
        const vehicleSelect = document.getElementById('vehicle-assignment');
        if (!vehicleSelect) return;
        
        // Remove existing listener to avoid duplicates
        vehicleSelect.removeEventListener('change', this.handleVehicleSelection);
        
        // Add new listener
        this.handleVehicleSelection = async (event) => {
            const selectedVehicleId = event.target.value;
            const pickupContainer = document.getElementById('pickup-location-container');
            
            if (selectedVehicleId && this.vehicleLocations && this.vehicleLocations[selectedVehicleId]) {
                const location = this.vehicleLocations[selectedVehicleId];
                
                // Show pickup location container
                pickupContainer.classList.remove('hidden');
                
                // Auto-populate pickup location with predicted vehicle location
                document.getElementById('pickup-lat').value = location.lat;
                document.getElementById('pickup-lng').value = location.lng;
                
                // Convert coordinates to address using reverse geocoding
                this.reverseGeocode(location.lat, location.lng, 'pickup-address');
                
                console.log(`Auto-populated pickup location for vehicle ${selectedVehicleId}:`, location);
            } else {
                // Hide pickup location if no vehicle selected
                pickupContainer.classList.add('hidden');
                
                // Clear pickup location data
                document.getElementById('pickup-address').value = '';
                document.getElementById('pickup-lat').value = '';
                document.getElementById('pickup-lng').value = '';
            }
            
            // Show/hide and auto-populate certification requirements based on selected vehicle
            const certificationContainer = document.getElementById('certification-requirements-container');
            if (selectedVehicleId) {
                // Show certification requirements section
                certificationContainer.classList.remove('hidden');
                await this.loadVehicleCertificationRequirements(selectedVehicleId);
            } else {
                // Hide certification requirements section when no vehicle selected
                certificationContainer.classList.add('hidden');
                this.clearCertificationRequirements();
            }
        };
        
        vehicleSelect.addEventListener('change', this.handleVehicleSelection);
    }
    
    async loadVehicleCertificationRequirements(vehicleId) {
        try {
            // Query vehicle certification requirements via requires-certification relations
            const response = await fetch(`${this.apiBaseUrl}/vehicles`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `
                        match
                        $vehicle isa vehicle, has vehicle-id "${vehicleId}";
                        $req_rel isa requires-certification (requiring-vehicle: $vehicle, required-certification: $cert);
                        $cert has certification-name $cert_name;
                        select $cert_name;
                    `
                })
            });

            if (response.ok) {
                const data = await response.json();
                if (data.ok && data.ok.answers && data.ok.answers.length > 0) {
                    // Extract required certification names
                    const requiredCertifications = data.ok.answers
                        .map(answer => answer.data.cert_name?.value)
                        .filter(cert => cert);
                    
                    // Auto-check the required certification checkboxes
                    this.updateCertificationCheckboxes(requiredCertifications);
                    
                    console.log(`Auto-populated certification requirements for vehicle ${vehicleId}:`, requiredCertifications);
                } else {
                    // No certification requirements for this vehicle
                    this.clearCertificationRequirements();
                    console.log(`No certification requirements found for vehicle ${vehicleId}`);
                }
            }
        } catch (error) {
            console.error(`Error loading certification requirements for vehicle ${vehicleId}:`, error);
            this.clearCertificationRequirements();
        }
    }
    
    updateCertificationCheckboxes(requiredCertifications) {
        // First, clear all checkboxes
        this.clearCertificationRequirements();
        
        // Then check the required ones and disable all checkboxes
        const checkboxes = document.querySelectorAll('.certification-checkbox');
        checkboxes.forEach(checkbox => {
            if (requiredCertifications.includes(checkbox.value)) {
                checkbox.checked = true;
            }
            // Disable all checkboxes since they're system-controlled
            checkbox.disabled = true;
        });
    }
    
    clearCertificationRequirements() {
        // Uncheck all certification checkboxes and re-enable them for manual selection
        const checkboxes = document.querySelectorAll('.certification-checkbox');
        checkboxes.forEach(checkbox => {
            checkbox.checked = false;
            checkbox.disabled = false; // Re-enable for manual selection when no vehicle selected
        });
    }
    
    async reverseGeocode(lat, lng, inputId) {
        try {
            // Convert string coordinates to numbers
            const numLat = parseFloat(lat);
            const numLng = parseFloat(lng);
            
            // Use TomTom backend service for reverse geocoding
            const response = await fetch(`${this.apiBaseUrl}/tomtom`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'reverse-geocode'
                },
                body: JSON.stringify({
                    lat: numLat,
                    lng: numLng
                })
            });
            
            if (response.ok) {
                const data = await response.json();
                console.log('TomTom reverse geocode response:', data);
                
                if (data.status === 'success' && data.address) {
                    const addressInput = document.getElementById(inputId);
                    if (addressInput) {
                        addressInput.value = data.address;
                    }
                    return data.address;
                } else {
                    throw new Error('Invalid reverse geocoding response');
                }
            } else {
                throw new Error(`Reverse geocoding failed: ${response.status}`);
            }
            
        } catch (error) {
            console.warn('Reverse geocoding failed:', error);
            // Fallback to coordinates
            const numLat = parseFloat(lat) || 0;
            const numLng = parseFloat(lng) || 0;
            const address = `${numLat.toFixed(4)}, ${numLng.toFixed(4)}`;
            const addressInput = document.getElementById(inputId);
            if (addressInput) {
                addressInput.value = address;
            }
            return address;
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
            
            // Get required certifications from the form
            const requiredCerts = Array.from(document.querySelectorAll('.certification-checkbox:checked')).map(cb => cb.value);
            
            // Driver Selection Query: Find up to 15 legally compliant drivers with required certifications who will be near the pickup location at the pickup time
            // 
            // Query Logic:
            // 1. Filter by jurisdiction-specific hours compliance (compliant_employees function)
            // 2. Filter by required certifications using has-certification relations
            // 3. Predict driver location at pickup time using disjunction:
            //    - Case A: Driver idle → use current position (current-lat, current-lng)
            //    - Case B: Driver busy → use delivery destination (dest-lat, dest-lng)
            // 4. Apply geographic bounding box filter for proximity
            // 5. Return candidate drivers for JavaScript scoring (distance, performance, fatigue, etc.)
            
            let certificationFilter = '';
            if (requiredCerts.length > 0) {
                // Build certification filter - driver must have ALL required certifications
                const certFilters = requiredCerts.map(cert => 
                    `$cert_rel_${cert.replace('-', '_')} isa has-certification (certified-employee: $compliant, held-certification: $cert_${cert.replace('-', '_')}); $cert_${cert.replace('-', '_')} has certification-name "${cert}";`
                ).join(' ');
                certificationFilter = certFilters;
            }
            
            const response = await fetch(`${this.apiBaseUrl}/employees`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'x-service-method': 'read'
                },
                body: JSON.stringify({
                    query: `match let $compliant in compliant_employees(${maxHours}); $compliant has id $id, has employee-name $name, has employee-role "driver", has daily-hours $hours, has performance-rating $rating; ${certificationFilter} { $compliant has current-lat $lat, has current-lng $lng; not { $assignment1 isa assignment (assigned-employee: $compliant, assigned-delivery: $delivery1), has assigned-at $assignTime1; $assignTime1 == "${isoDateTime}"; }; } or { $assignment2 isa assignment (assigned-employee: $compliant, assigned-delivery: $delivery2), has assigned-at $assignTime2; $assignTime2 == "${isoDateTime}"; $delivery2 has dest-lat $lat, has dest-lng $lng; }; $lat > ${minLat}; $lat < ${maxLat}; $lng > ${minLng}; $lng < ${maxLng}; select $compliant, $id, $name, $lat, $lng, $hours, $rating; limit 15;`
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
                    const driverCertifications = '';
                    
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
            const customerNameEl = document.getElementById('customer-name');
            const packageWeightEl = document.getElementById('package-weight');
            const customerName = customerNameEl ? customerNameEl.value.trim() : '';
            const packageWeight = packageWeightEl ? packageWeightEl.value : '';
            
            const submitBtn = document.getElementById('assignment-submit-btn');
            const driverContainer = document.getElementById('driver-assignment-container');
            
            // Check if all required fields except driver are filled
            // Note: pickup fields are auto-populated when vehicle is selected, so we check if they exist
            const allFieldsExceptDriverFilled = routeId && 
                                               scheduleTime &&
                                               vehicleId && 
                                               pickupAddress && 
                                               pickupLat && 
                                               pickupLng && 
                                               deliveryAddress &&
                                               deliveryLat &&
                                               deliveryLng &&
                                               deliveryPriority;
            
            // Show/hide driver field based on other fields
            if (allFieldsExceptDriverFilled) {
                driverContainer.classList.remove('hidden');
            } else {
                driverContainer.classList.add('hidden');
            }
            
            // Check if all required fields including driver are filled
            const allFieldsFilled = allFieldsExceptDriverFilled && driverId;
            
            if (allFieldsFilled) {
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
                    query: 'match $e isa employee, has id $id, has employee-name $name, has employee-role "driver", has employee-status "available", has daily-hours $hours; $hours < 11.0; not { $assignment isa assignment (assigned-employee: $e, assigned-vehicle: $vehicle); }; select $e, $id, $name; limit 10;'
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
    
    
    
}

// Initialize FleetCommand Pro when DOM is ready
document.addEventListener('DOMContentLoaded', () => {
    console.log('🚀 Starting FleetCommand Pro Enterprise System...');
    window.fleetCommand = new FleetCommandPro();
});
