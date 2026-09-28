const { spawn } = require('node:child_process');
const assert = require('node:assert/strict');
const proc = spawn('./target/debug/iot-devices', [], {stdio: ['ignore', 'ignore', 'pipe']});
(async () => {
  let socket;
  try {
    for(let i=0;i<40;i++) {try {if((await fetch('http://127.0.0.1:3000/api/devices')).ok)break;}catch{} await new Promise(r=>setTimeout(r,100));}
    socket = new WebSocket('ws://127.0.0.1:3000/ws');
    await new Promise((resolve,reject)=> {socket.onopen=resolve;socket.onerror=reject;setTimeout(()=>reject(new Error('open timeout')),5000).unref();});
    const received = new Promise((resolve,reject)=> {socket.onmessage=e=>resolve(JSON.parse(e.data));setTimeout(()=>reject(new Error('broadcast timeout')),5000).unref();});
    const response = await fetch('http://127.0.0.1:3000/api/devices',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({id:'smoke','name':'Test device','type':'light','value':'off'})});
    assert.equal(response.ok,true);
    const event = await received;
    assert.equal(event.type,'BROADCAST');
    assert.equal(event.payload.id,'smoke');
    console.log('IoT HTTP mutation -> WebSocket broadcast passed');
  } finally {socket?.close();proc.kill('SIGINT');}
})().catch(e=>{console.error(e);process.exitCode=1;});
