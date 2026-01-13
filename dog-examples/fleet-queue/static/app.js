class FleetQueueManager {
    constructor() {
        this.map = null;
        this.markers = new Map();
        this.sidebarCollapsed = false;
        this.cardExpanded = false;
        this.currentVehicle = null;
        
        // Fleet data
        this.fleetData = {
            vehicles: [
                {
                    id: 'FL-2045',
                    driver: {
                        name: 'Devon Lindsay',
                        avatar: 'https://images.unsplash.com/photo-1507003211169-0a1dd7228f2d?w=40&h=40&fit=crop&crop=face'
                    },
                    status: 'active',
                    route: {
                        from: 'Istanbul Distribution Center',
                        to: 'Utah Logistics Hub',
                        eta: '18h 30m',
                        distance: '11,247 km'
                    },
                    location: {
                        lat: 41.0082,
                        lng: 28.9784
                    }
                },
                {
                    id: 'FL-2046',
                    driver: {
                        name: 'Sarah Chen',
                        avatar: 'https://images.unsplash.com/photo-1494790108755-2616b612b786?w=40&h=40&fit=crop&crop=face'
                    },
                    status: 'active',
                    route: {
                        from: 'Berlin Hub',
                        to: 'Warsaw Center',
                        eta: '2h 45m',
                        distance: '573 km'
                    },
                    location: {
                        lat: 52.2297,
                        lng: 21.0122
                    }
                }
            ]
        };
        
        this.init();
    }
    
    init() {
        this.setupEventListeners();
        this.initializeMap();
        this.loadFleetData();
    }
    
    setupEventListeners() {
        // Mobile menu toggle
        const mobileMenuToggle = document.getElementById('mobileMenuToggle');
        if (mobileMenuToggle) {
            mobileMenuToggle.addEventListener('click', () => this.toggleMobileSidebar());
        }
        
        // Sidebar toggle (desktop)
        const sidebarToggle = document.getElementById('sidebarToggle');
        if (sidebarToggle) {
            sidebarToggle.addEventListener('click', () => this.toggleSidebar());
        }
        
        
        // Contact buttons
        const contactBtns = document.querySelectorAll('.contact-btn');
        contactBtns.forEach(btn => {
            btn.addEventListener('click', (e) => this.handleContact(e));
        });
        
        // Responsive sidebar for mobile
        this.setupResponsiveSidebar();
    }
    
    toggleSidebar() {
        const sidebar = document.getElementById('sidebar');
        this.sidebarCollapsed = !this.sidebarCollapsed;
        
        if (this.sidebarCollapsed) {
            sidebar.classList.add('collapsed');
        } else {
            sidebar.classList.remove('collapsed');
        }
        
        // Resize map after sidebar animation
        setTimeout(() => {
            if (this.map) {
                this.map.getMap().resize();
            }
        }, 300);
    }
    
    toggleMobileSidebar() {
        const sidebar = document.getElementById('sidebar');
        sidebar.classList.toggle('open');
    }
    
    
    handleContact(e) {
        const contactType = e.currentTarget.classList.contains('phone') ? 'phone' : 'message';
        console.log(`Initiating ${contactType} contact`);
        
        if (contactType === 'phone') {
            // Simulate phone call
            alert('Calling driver...');
        } else {
            // Simulate message
            alert('Opening message interface...');
        }
    }
    
    setupResponsiveSidebar() {
        // Close sidebar when clicking outside on mobile
        document.addEventListener('click', (e) => {
            if (window.innerWidth <= 768) {
                const sidebar = document.getElementById('sidebar');
                const mobileMenuToggle = document.getElementById('mobileMenuToggle');
                
                if (!sidebar.contains(e.target) && !mobileMenuToggle.contains(e.target)) {
                    sidebar.classList.remove('open');
                }
            }
        });
    }
    
    initializeMap() {
        console.log('Initializing TomTom map with API key: c1xp5uxxF9W7z0tPjNcQC48nQlABojKH');
        
        // Check if container exists
        const mapContainer = document.getElementById('mapView');
        if (!mapContainer) {
            console.error('Map container #mapView not found');
            this.initializeFallbackMap();
            return;
        }
        
        // Check if TomTom SDK is loaded
        if (typeof tt === 'undefined') {
            console.error('TomTom SDK not loaded');
            this.initializeFallbackMap();
            return;
        }
        
        try {
            // Initialize TomTom map with correct API key and English language
            this.map = tt.map({
                key: 'c1xp5uxxF9W7z0tPjNcQC48nQlABojKH',
                container: 'mapView',
                center: [0, 0], // World center coordinates
                zoom: 1,
                language: 'en-US'
            });
            
            // Add map controls
            this.map.addControl(new tt.NavigationControl());
            
            // Map loaded event
            this.map.on('load', () => {
                console.log('TomTom map loaded successfully');
                this.addFleetMarkers();
            });
            
            // Map error event
            this.map.on('error', (error) => {
                console.error('TomTom map error:', error);
                this.initializeFallbackMap();
            });
            
        } catch (error) {
            console.error('TomTom map initialization failed:', error);
            this.initializeFallbackMap();
        }
    }
    
    initializeFallbackMap() {
        console.log('Initializing fallback map');
        const mapView = document.getElementById('mapView');
        mapView.innerHTML = `
            <div style="
                width: 100%;
                height: 100%;
                background: linear-gradient(135deg, #1e293b 0%, #334155 100%);
                display: flex;
                align-items: center;
                justify-content: center;
                color: #64748b;
                font-size: 18px;
                text-align: center;
                padding: 40px;
            ">
                <div>
                    <div style="font-size: 48px; margin-bottom: 16px;">🗺️</div>
                    <div>Map Loading...</div>
                    <div style="font-size: 14px; margin-top: 8px; opacity: 0.7;">
                        TomTom map will appear here
                    </div>
                </div>
            </div>
        `;
    }
    
    addFleetMarkers() {
        if (!this.map) return;
        
        this.fleetData.vehicles.forEach(vehicle => {
            // Create marker
            const marker = new tt.Marker()
                .setLngLat([vehicle.location.lng, vehicle.location.lat])
                .addTo(this.map);
            
            // Store marker reference
            this.markers.set(vehicle.id, marker);
            
            // Create popup
            const popup = new tt.Popup({ offset: 35 }).setHTML(`
                <div style="padding: 8px;">
                    <strong>${vehicle.id}</strong><br>
                    Driver: ${vehicle.driver.name}<br>
                    Route: ${vehicle.route.from} → ${vehicle.route.to}<br>
                    ETA: ${vehicle.route.eta}
                </div>
            `);
            
            marker.setPopup(popup);
        });
    }
    
    loadFleetData() {
        // Simulate loading fleet data
        const sidebar = document.getElementById('sidebar');
        if (!sidebar) return;
        
        // Fleet data is already loaded in constructor
        console.log('Fleet data loaded:', this.fleetData);
    }
    
    // Navigation methods
    showFleetOverview() {
        // Show default fleet overview (current view)
        document.querySelector('.fleet-list').style.display = 'flex';
        document.querySelector('.order-info').style.display = 'block';
        console.log('Showing fleet overview');
    }
    
    showGridView() {
        // Toggle to grid view of fleet items
        const fleetList = document.querySelector('.fleet-list');
        if (fleetList.classList.contains('grid-view')) {
            fleetList.classList.remove('grid-view');
        } else {
            fleetList.classList.add('grid-view');
        }
        console.log('Toggled grid view');
    }
    
    showDocuments() {
        // Show documents/reports view
        console.log('Showing documents view');
        alert('Documents view - Feature coming soon!');
    }
    
    showSettings() {
        // Show settings panel
        console.log('Showing settings');
        alert('Settings - Feature coming soon!');
    }
}

// Initialize the application when DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    console.log('DOM loaded, initializing Fleet Queue Manager...');
    new FleetQueueManager();
});
