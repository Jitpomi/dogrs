// Exercise the actual demo protocol handlers without a browser or UI framework.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = path.resolve(__dirname, '../..');

const iot = fs.readFileSync(path.join(root, 'dog-examples/iot-devices/static/app.js'), 'utf8');
const handlers = iot.slice(iot.indexOf('function sendWSRequest('), iot.indexOf('// Request real-time stats'));
vm.runInNewContext(`
    let inFlightRequest = null;
    let activeRequests = new Map();
    const sent = [];
    const errors = [];
    let socket = {readyState: 1, send: value => sent.push(JSON.parse(value))};
    const WebSocket = {OPEN: 1};
    function logToConsole() {}
    ${handlers}
    let completed = false;
    sendWSRequest('Create', {}, () => {completed = true;});
    sendWSRequest('Create', {}, result => errors.push(result.error));
    assert.equal(sent.length, 1);
    assert.equal(errors.length, 1);
    assert.equal(completed, false);
    handleWSResponse({request_id: 'unknown'});
    assert.equal(inFlightRequest, sent[0].request_id);
    handleWSResponse({request_id: sent[0].request_id, payload: {ok: true}});
    assert.equal(completed, true);
    assert.equal(activeRequests.size, 0);
    assert.equal(inFlightRequest, null);
    socket.send = () => {throw new Error('disconnected');};
    sendWSRequest('Create', {}, result => errors.push(result.error));
    assert.equal(activeRequests.size, 0);
    assert.equal(inFlightRequest, null);
    assert.equal(errors.length, 2);
    socket.readyState = 3;
    sendWSRequest('Create', {}, result => errors.push(result.error));
    assert.equal(errors.length, 3);
`, {assert});

const fleet = fs.readFileSync(path.join(root, 'dog-examples/fleet-queue/static/app.js'), 'utf8');
const method = fleet.slice(fleet.indexOf('    startRealTimeUpdates() {'), fleet.indexOf('    setupDeliveryDetailsButton() {'));
const instances = [];
const timers = [];
class EventSource {
    constructor() {this.handlers = {}; instances.push(this);}
    addEventListener(name, fn) {this.handlers[name] = fn;}
    close() {this.closed = true;}
}
const controller = vm.runInNewContext(`({${method}})`, {
    EventSource, console: {log() {}, error() {}},
    clearTimeout() {}, setTimeout(fn) {timers.push(fn); return timers.length;},
});
let loads = 0;
let release;
controller.loadAllData = async () => {
    loads++;
    if (loads === 1) await new Promise(resolve => {release = resolve;});
};
controller.addVehicleMarkers = async () => {};
controller.updateUI = () => {};

(async () => {
    controller.startRealTimeUpdates();
    const source = instances[0];
    source.onopen();
    for (let i = 0; i < 100; i++) source.onmessage();
    assert.equal(loads, 1, 'coalesce events instead of starting 100 requests');
    release();
    await controller.realTimeRefresh;
    assert.equal(loads, 2, 'refresh again for changes received during the snapshot');
    await source.handlers['dogrs.stream_error']();
    assert.equal(source.closed, true);
    assert.equal(loads, 3);
    assert.equal(timers.length, 1);
    timers[0]();
    assert.equal(instances.length, 2);
    instances[1].onopen();
    await controller.realTimeRefresh;
    assert.equal(loads, 4, 'reconnect always reloads authoritative state');
    console.log('Realtime example protocol tests passed');
})().catch(error => {console.error(error); process.exitCode = 1;});
