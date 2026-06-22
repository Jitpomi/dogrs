// State variables
let socket = null;
let reconnectTimer = null;
let statsTimer = null;
let activeRequests = new Map();
let devices = [];

// DOM elements
const wsStatusBadge = document.getElementById('ws-status');
const devicesGrid = document.getElementById('devices-grid');
const consoleLogs = document.getElementById('console-logs');
const btnClearConsole = document.getElementById('btn-clear-console');
const btnAddDevice = document.getElementById('btn-add-device');
const modalContainer = document.getElementById('modal-container');
const btnCloseModal = document.getElementById('btn-close-modal');
const btnCancel = document.getElementById('btn-cancel');
const addDeviceForm = document.getElementById('add-device-form');
const selectDeviceType = document.getElementById('device-type');
const initialValGroup = document.getElementById('initial-val-group');

const statTotal = document.getElementById('stat-total');
const statOnline = document.getElementById('stat-online');
const statTemp = document.getElementById('stat-temp');

// Log message to terminal console with optional pretty-printed JSON payload
function logToConsole(message, payload = null, type = 'system') {
    const line = document.createElement('div');
    line.className = `log-line ${type}`;
    const timestamp = new Date().toISOString().split('T')[1].slice(0, -1);
    
    const headerSpan = document.createElement('span');
    headerSpan.className = 'log-header';
    headerSpan.innerText = `[${timestamp}] ${message}`;
    line.appendChild(headerSpan);
    
    if (payload !== null) {
        const pre = document.createElement('pre');
        pre.className = 'log-payload';
        pre.innerText = typeof payload === 'string' ? payload : JSON.stringify(payload, null, 2);
        line.appendChild(pre);
    }
    
    consoleLogs.appendChild(line);
    consoleLogs.scrollTop = consoleLogs.scrollHeight;
}

// Clear logs
btnClearConsole.addEventListener('click', () => {
    consoleLogs.innerHTML = '';
    logToConsole('Console logs cleared.', null, 'system');
});

// Fetch all devices (REST API)
async function fetchDevices() {
    logToConsole('HTTP GET -> /api/devices', null, 'http-get');
    try {
        const res = await fetch('/api/devices', {
            headers: { 'x-tenant-id': 'default' }
        });
        devices = await res.json();
        logToConsole(`HTTP GET response received (Status: ${res.status})`, devices, 'ws-recv');
        renderDevices(devices);
        calculateStatsLocal(devices);
    } catch (err) {
        logToConsole(`HTTP GET failed: ${err.message}`, null, 'error');
        devicesGrid.innerHTML = `
            <div class="loading-state">
                <p class="error">Failed to connect to REST API backend.</p>
            </div>
        `;
    }
}

// Render Devices list
function renderDevices(devices) {
    if (devices.length === 0) {
        devicesGrid.innerHTML = `
            <div class="loading-state">
                <p>No registered devices. Add a device to start monitoring.</p>
            </div>
        `;
        return;
    }

    devicesGrid.innerHTML = '';
    devices.forEach(dev => {
        const card = document.createElement('div');
        const isOff = dev.value === 'off' || dev.value === 'unlocked';
        card.className = `device-card glass-panel ${dev.type} ${isOff ? 'off' : ''}`;
        card.id = `device-card-${dev.id}`;

        let icon = '';
        let controls = '';

        if (dev.type === 'light') {
            icon = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .6 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5M9 18h6M10 22h4"/></svg>`;
            const checked = dev.value === 'on' ? 'checked' : '';
            controls = `
                <span class="status-text">${dev.value.toUpperCase()}</span>
                <label class="switch">
                    <input type="checkbox" ${checked} onchange="handleToggle('${dev.id}', this.checked ? 'on' : 'off')">
                    <span class="slider"></span>
                </label>
            `;
        } else if (dev.type === 'lock') {
            const isLocked = dev.value === 'locked';
            icon = isLocked 
                ? `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>`
                : `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/></svg>`;
            
            const checked = isLocked ? 'checked' : '';
            controls = `
                <span class="status-text">${dev.value.toUpperCase()}</span>
                <label class="switch">
                    <input type="checkbox" ${checked} onchange="handleToggle('${dev.id}', this.checked ? 'locked' : 'unlocked')">
                    <span class="slider"></span>
                </label>
            `;
        } else if (dev.type === 'thermostat') {
            icon = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 14.76V3.5a2.5 2.5 0 0 0-5 0v11.26a4.5 4.5 0 1 0 5 0z"/></svg>`;
            controls = `
                <span class="status-text">${parseFloat(dev.value).toFixed(1)}°C</span>
                <div class="temp-controls">
                    <button class="btn-round" onclick="adjustTemp('${dev.id}', -0.5)">&minus;</button>
                    <button class="btn-round" onclick="adjustTemp('${dev.id}', 0.5)">&plus;</button>
                </div>
            `;
        }

        card.innerHTML = `
            <div class="device-header">
                <div class="device-title-wrap">
                    <span class="device-name">${dev.name}</span>
                    <span class="device-type-label">${dev.type}</span>
                </div>
                <div class="device-icon-container">
                    ${icon}
                </div>
            </div>
            <button class="delete-btn" onclick="deleteDevice('${dev.id}')" title="Remove device">&times;</button>
            <div class="device-control">
                ${controls}
            </div>
        `;

        devicesGrid.appendChild(card);
    });
}

// Local stats computation as a fallback/initial step
function calculateStatsLocal(devices) {
    statTotal.innerText = devices.length;
    const onlineCount = devices.filter(d => d.status === 'online').length;
    statOnline.innerText = `${onlineCount}/${devices.length}`;
    
    const temps = devices.filter(d => d.type === 'thermostat').map(d => parseFloat(d.value));
    if (temps.length === 0) {
        statTemp.innerText = '-';
    } else {
        const avg = temps.reduce((a, b) => a + b, 0) / temps.length;
        statTemp.innerText = `${avg.toFixed(1)}°C`;
    }
}

// WebSocket Connection Setup
function setupWebSocket() {
    const wsProto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${wsProto}//${window.location.host}/ws`;
    
    logToConsole(`Connecting to WebSocket server at ${wsUrl}...`, null, 'system');
    wsStatusBadge.className = 'status-badge ws connecting';
    
    socket = new WebSocket(wsUrl);
    
    socket.onopen = () => {
        logToConsole('WebSocket connection established.', null, 'system');
        wsStatusBadge.className = 'status-badge ws connected';
        requestStats();
    };
    
    socket.onmessage = (event) => {
        try {
            const data = JSON.parse(event.data);
            if (data.type === 'BROADCAST') {
                handleWSBroadcast(data);
            } else {
                const reqIdStr = data.request_id ? ` (${data.request_id})` : '';
                logToConsole(`WS RECV${reqIdStr}`, data, 'ws-recv');
                if (data.type === 'RESPONSE') {
                    handleWSResponse(data);
                }
            }
        } catch (err) {
            logToConsole(`WS RECV Raw: ${event.data}`, null, 'ws-recv');
            logToConsole(`WS Parse Error: ${err.message}`, null, 'error');
        }
    };
    
    socket.onclose = () => {
        logToConsole('WebSocket connection closed.', null, 'error');
        wsStatusBadge.className = 'status-badge ws disconnected';
        
        // Reconnect logic
        if (!reconnectTimer) {
            reconnectTimer = setTimeout(() => {
                reconnectTimer = null;
                setupWebSocket();
            }, 3000);
        }
    };
    
    socket.onerror = (err) => {
        logToConsole(`WebSocket error occurred.`, null, 'error');
    };
}

// Handle WebSocket real-time broadcast events
function handleWSBroadcast(broadcast) {
    const { event, payload } = broadcast;
    logToConsole(`WS BROADCAST [${event}]`, payload, 'ws-recv');
    
    if (event === 'created') {
        if (!devices.some(d => d.id === payload.id)) {
            devices.push(payload);
        }
    } else if (event === 'removed') {
        devices = devices.filter(d => d.id !== payload.id);
    } else {
        // updated, toggle, telemetry, etc.
        const idx = devices.findIndex(d => d.id === payload.id);
        if (idx !== -1) {
            devices[idx] = payload;
        } else {
            devices.push(payload);
        }
    }
    
    renderDevices(devices);
    calculateStatsLocal(devices);
}

// Send request via WebSocket
function sendWSRequest(method, payload, callback) {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
        logToConsole('Cannot send WebSocket request: connection closed.', null, 'error');
        return;
    }
    
    const requestId = `req-${Date.now()}-${Math.random().toString(36).substr(2, 5)}`;
    const methodStr = typeof method === 'string' ? method : (method.Custom || Object.keys(method)[0]);
    
    const wsReq = {
        type: 'REQUEST',
        request_id: requestId,
        transport: 'WebSocket',
        service: 'devices',
        method: method,
        tenant: { tenant_id: 'default' },
        params: {
            provider: 'websocket',
            headers: {},
            query: {},
            method: 'POST',
            path: `/devices/${methodStr}`
        },
        payload: payload,
        metadata: {}
    };
    
    if (callback) {
        activeRequests.set(requestId, callback);
    }
    
    logToConsole(`WS SEND (${requestId})`, wsReq, 'ws-sent');
    socket.send(JSON.stringify(wsReq));
}

// Handle WebSocket responses
function handleWSResponse(res) {
    const callback = activeRequests.get(res.request_id);
    if (callback) {
        activeRequests.delete(res.request_id);
        callback(res);
    } else {
        // Broadcast responses or un-tracked updates
        if (res.error) {
            logToConsole(`Action error returned: ${res.error}`, null, 'error');
        }
    }
}

// Request real-time stats via WebSocket
function requestStats() {
    sendWSRequest({ Custom: 'stats' }, {}, (res) => {
        if (res.error) {
            logToConsole(`Failed to fetch stats: ${res.error}`, null, 'error');
            return;
        }
        
        const stats = res.payload;
        statTotal.innerText = stats.total_devices;
        statOnline.innerText = `${stats.online_devices}/${stats.total_devices}`;
        statTemp.innerText = stats.average_temperature > 0 
            ? `${parseFloat(stats.average_temperature).toFixed(1)}°C` 
            : '-';
    });
}

// Toggle control switch action (Light or Lock)
function handleToggle(id, newValue) {
    sendWSRequest({ Custom: 'toggle' }, { id }, (res) => {
        if (res.error) {
            logToConsole(`Toggle operation failed: ${res.error}`, null, 'error');
            // Refresh grid to reset switch state to actual DB state
            fetchDevices();
        } else {
            const updatedDev = res.payload;
            logToConsole(`Toggle success: ${updatedDev.name} is now ${updatedDev.value}`, null, 'system');
        }
    });
}

// Adjust Thermostat Temperature
function adjustTemp(id, delta) {
    // We first read local card status to calculate target temperature
    const card = document.getElementById(`device-card-${id}`);
    if (!card) return;
    
    const textNode = card.querySelector('.status-text');
    if (!textNode) return;
    
    const currentVal = parseFloat(textNode.innerText);
    const newVal = currentVal + delta;
    
    // Update local card instantly for snappier UI
    textNode.innerText = `${newVal.toFixed(1)}°C`;
    
    sendWSRequest({ Custom: 'telemetry' }, { id, value: newVal }, (res) => {
        if (res.error) {
            logToConsole(`Temperature adjustment failed: ${res.error}`, null, 'error');
            fetchDevices();
        } else {
            const updated = res.payload;
            logToConsole(`Temperature adjustment success: ${updated.name} set to ${updated.value}°C`, null, 'system');
        }
    });
}

// Delete device (REST API DELETE request)
async function deleteDevice(id) {
    if (!confirm('Are you sure you want to remove this device?')) return;
    
    logToConsole(`HTTP DELETE -> /api/devices/${id}`, null, 'http-get');
    try {
        const res = await fetch(`/api/devices/${id}`, {
            method: 'DELETE',
            headers: { 'x-tenant-id': 'default' }
        });
        
        if (res.status === 200) {
            const deleted = await res.json();
            logToConsole(`HTTP DELETE success (Status: ${res.status})`, deleted, 'ws-recv');
        } else {
            logToConsole(`HTTP DELETE failed with status: ${res.status}`, null, 'error');
        }
    } catch (err) {
        logToConsole(`HTTP DELETE failed: ${err.message}`, null, 'error');
    }
}

// Register modal UI controls
btnAddDevice.addEventListener('click', () => {
    modalContainer.classList.add('open');
});

function closeModal() {
    modalContainer.classList.remove('open');
    addDeviceForm.reset();
    updateValPlaceholder();
}

btnCloseModal.addEventListener('click', closeModal);
btnCancel.addEventListener('click', closeModal);

// Change placeholders dynamically based on selected device type
function updateValPlaceholder() {
    const type = selectDeviceType.value;
    const valInput = document.getElementById('device-value');
    if (type === 'thermostat') {
        valInput.placeholder = 'e.g. 21.5 (temperature float)';
        valInput.type = 'number';
        valInput.step = '0.1';
        valInput.min = '5';
        valInput.max = '35';
        valInput.value = '21.0';
    } else if (type === 'light') {
        valInput.placeholder = 'on or off';
        valInput.type = 'text';
        valInput.value = 'off';
    } else if (type === 'lock') {
        valInput.placeholder = 'locked or unlocked';
        valInput.type = 'text';
        valInput.value = 'locked';
    }
}

selectDeviceType.addEventListener('change', updateValPlaceholder);

// Submit device form (REST API POST request)
addDeviceForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    
    const name = document.getElementById('device-name').value;
    const type = selectDeviceType.value;
    let valueRaw = document.getElementById('device-value').value;
    
    // Parse value appropriately
    let value = valueRaw;
    if (type === 'thermostat') {
        value = parseFloat(valueRaw);
    }
    
    const payload = {
        name,
        type,
        value,
        status: 'online'
    };
    
    logToConsole(`HTTP POST -> /api/devices`, payload, 'http-post');
    try {
        const res = await fetch('/api/devices', {
            method: 'POST',
            headers: {
                'content-type': 'application/json',
                'x-tenant-id': 'default'
            },
            body: JSON.stringify(payload)
        });
        
        if (res.ok) {
            const created = await res.json();
            logToConsole(`HTTP POST success (Status: ${res.status})`, created, 'ws-recv');
            closeModal();
        } else {
            const errText = await res.text();
            logToConsole(`HTTP POST failed (Status: ${res.status}): ${errText}`, null, 'error');
        }
    } catch (err) {
        logToConsole(`HTTP POST failed: ${err.message}`, null, 'error');
    }
});

// App initialization
window.addEventListener('load', () => {
    updateValPlaceholder();
    fetchDevices();
    setupWebSocket();
});
