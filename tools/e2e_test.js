#!/usr/bin/env node
/**
 * LightSpeed E2E Tunnel Test (Node.js)
 *
 * Tests the full data path:
 *   Client (BKK) → Proxy (LA/OCI/SGP) → Game Server (SGP echo) → Proxy → Client
 *
 * Encodes LightSpeed tunnel headers matching protocol/src/header.rs format.
 */

const dgram = require('dgram');
const { Buffer } = require('buffer');
const { performance } = require('perf_hooks');

// ── Header format (20 bytes) ────────────────────────────────────
const PROTOCOL_VERSION = 1;
const HEADER_SIZE = 20;
const FLAG_KEEPALIVE = 0x01;

function ipToBytes(ip) {
  return Buffer.from(ip.split('.').map(Number));
}

function bytesToIp(buf, offset) {
  return `${buf[offset]}.${buf[offset+1]}.${buf[offset+2]}.${buf[offset+3]}`;
}

function nowUs() {
  return (Date.now() * 1000) >>> 0;
}

function encodeHeader(seq, timestampUs, srcIp, srcPort, dstIp, dstPort, flags = 0, sessionToken = 0) {
  const buf = Buffer.alloc(HEADER_SIZE);
  buf[0] = (PROTOCOL_VERSION << 4) | (flags & 0x0F);
  buf[1] = sessionToken;
  buf.writeUInt16BE(seq, 2);
  buf.writeUInt32BE(timestampUs, 4);
  ipToBytes(srcIp).copy(buf, 8);
  ipToBytes(dstIp).copy(buf, 12);
  buf.writeUInt16BE(srcPort, 16);
  buf.writeUInt16BE(dstPort, 18);
  return buf;
}

function decodeHeader(data) {
  if (data.length < HEADER_SIZE) throw new Error(`Too short: ${data.length} < ${HEADER_SIZE}`);
  const verFlags = data[0];
  return {
    header: {
      version: verFlags >> 4,
      flags: verFlags & 0x0F,
      sessionToken: data[1],
      sequence: data.readUInt16BE(2),
      timestampUs: data.readUInt32BE(4),
      srcIp: bytesToIp(data, 8),
      srcPort: data.readUInt16BE(16),
      dstIp: bytesToIp(data, 12),
      dstPort: data.readUInt16BE(18),
    },
    payload: data.slice(HEADER_SIZE),
  };
}

function testProxy(name, proxyHost, proxyPort, gameServerIp, gameServerPort) {
  return new Promise((resolve) => {
    console.log(`\n${'─'.repeat(50)}`);
    console.log(`Testing proxy: ${name} (${proxyHost}:${proxyPort})`);
    console.log(`Game server:   ${gameServerIp}:${gameServerPort}`);
    console.log('─'.repeat(50));

    const sock = dgram.createSocket('udp4');
    const results = { keepalive: null, rtts: [], errors: 0 };
    let phase = 'keepalive';
    let packetIdx = 0;
    const totalPackets = 5;
    let sentPayloads = [];
    let sendTimes = [];

    sock.on('error', (err) => {
      console.log(`  ❌ Socket error: ${err.message}`);
      results.errors++;
    });

    sock.on('message', (msg, rinfo) => {
      const recvTime = performance.now();

      try {
        const { header, payload } = decodeHeader(msg);

        if (phase === 'keepalive') {
          const rtt = recvTime - sendTimes[0];
          results.keepalive = rtt;
          console.log(`\n[1] Keepalive test:`);
          console.log(`  ✅ Keepalive echo: ${rtt.toFixed(1)}ms (seq=${header.sequence}, flags=0x${header.flags.toString(16).padStart(2,'0')})`);

          // Start data relay test
          phase = 'data';
          packetIdx = 0;
          sendTimes = [];
          sentPayloads = [];
          console.log(`\n[2] Data relay test (${totalPackets} packets):`);
          sendDataPacket();
          return;
        }

        if (phase === 'data') {
          const rtt = recvTime - sendTimes[packetIdx];
          results.rtts.push(rtt);

          const expectedPayload = sentPayloads[packetIdx];
          const match = payload.equals(expectedPayload);
          console.log(`  ${match ? '✅' : '❌'} Packet ${packetIdx}: ${rtt.toFixed(1)}ms, payload=${match ? 'MATCH' : 'MISMATCH'} (${payload.length}B), seq=${header.sequence}`);

          if (!match) {
            console.log(`      Sent:     ${expectedPayload.toString().slice(0, 40)}`);
            console.log(`      Received: ${payload.toString().slice(0, 40)}`);
          }

          packetIdx++;
          if (packetIdx < totalPackets) {
            setTimeout(sendDataPacket, 100);
          } else {
            finishTest();
          }
        }
      } catch (e) {
        console.log(`  ❌ Decode error: ${e.message}`);
        results.errors++;
      }
    });

    function sendDataPacket() {
      const payload = Buffer.from(`LIGHTSPEED_E2E_TEST_${packetIdx}_${Math.floor(Date.now()/1000)}`);
      sentPayloads[packetIdx] = payload;

      const header = encodeHeader(
        packetIdx, nowUs(),
        '127.0.0.1', 12345,
        gameServerIp, gameServerPort
      );

      const packet = Buffer.concat([header, payload]);
      sendTimes[packetIdx] = performance.now();
      sock.send(packet, proxyPort, proxyHost);

      // Timeout per packet
      setTimeout(() => {
        if (results.rtts.length <= packetIdx && phase === 'data') {
          console.log(`  ❌ Packet ${packetIdx}: TIMEOUT`);
          packetIdx++;
          if (packetIdx < totalPackets) {
            sendDataPacket();
          } else {
            finishTest();
          }
        }
      }, 5000);
    }

    let finished = false;
    function finishTest() {
      if (finished) return;
      finished = true;

      if (results.rtts.length > 0) {
        const avg = results.rtts.reduce((a,b) => a+b, 0) / results.rtts.length;
        const min = Math.min(...results.rtts);
        const max = Math.max(...results.rtts);
        console.log(`\n  📊 Results: avg=${avg.toFixed(1)}ms, min=${min.toFixed(1)}ms, max=${max.toFixed(1)}ms`);
        console.log(`  📊 Packets: ${results.rtts.length}/${totalPackets} received`);
        if (results.keepalive !== null) {
          const overhead = avg - results.keepalive;
          console.log(`  📊 Relay overhead vs keepalive: ${overhead >= 0 ? '+' : ''}${overhead.toFixed(1)}ms`);
        }
      } else {
        console.log(`\n  ❌ No responses received - relay may not be working`);
      }

      try { sock.close(); } catch(e) {}
      resolve(results);
    }

    // Start with keepalive test
    sock.bind(0, () => {
      const keepaliveHeader = encodeHeader(
        60000, nowUs(),
        '0.0.0.0', 0,
        '0.0.0.0', 0,
        FLAG_KEEPALIVE
      );

      sendTimes[0] = performance.now();
      sock.send(keepaliveHeader, proxyPort, proxyHost);

      // Keepalive timeout
      setTimeout(() => {
        if (results.keepalive === null) {
          console.log(`\n[1] Keepalive test:`);
          console.log(`  ❌ Keepalive: TIMEOUT (5s)`);

          // Still try data relay
          phase = 'data';
          packetIdx = 0;
          sendTimes = [];
          sentPayloads = [];
          console.log(`\n[2] Data relay test (${totalPackets} packets):`);
          sendDataPacket();
        }
      }, 5000);
    });
  });
}

async function main() {
  console.log('='.repeat(60));
  console.log('LightSpeed E2E Tunnel Test');
  console.log('='.repeat(60));

  const proxies = [
    ['Vultr LA',     '149.28.84.139',  4434],
    ['OCI San Jose', '163.192.3.134',  4434],
    ['Vultr SGP',    '149.28.144.74',  4434],
  ];
  const gameServerIp = '149.28.144.74';
  const gameServerPort = 9999;

  for (const [name, host, port] of proxies) {
    await testProxy(name, host, port, gameServerIp, gameServerPort);
  }

  console.log(`\n${'='.repeat(60)}`);
  console.log('Test complete');
  console.log('='.repeat(60));
}

main().catch(console.error);
