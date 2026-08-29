// LightSpeed community registry — Cloudflare Worker (reference implementation).
//
// Serves the signed node list and handles operator node registration and
// revocation. The registry payload is signed with the operator's Ed25519 key
// (stored as a secret) using WebCrypto; clients verify against the operator's
// public key (see client/src/registry.rs).
//
// NOT TESTED: this is a reference implementation. Deploy with `wrangler deploy`
// after running `wrangler secret put OPERATOR_KEY_B64` and `wrangler kv
// namespace create REGISTRY_KV` (then set the KV id in wrangler.toml).

const ED25519 = { name: 'Ed25519' };

function b64decode(str) {
  const bin = atob(str);
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

function b64encode(bytes) {
  return btoa(String.fromCharCode(...bytes));
}

async function signRegistry(registryJson, operatorKeyB64) {
  const keyBytes = b64decode(operatorKeyB64);
  const key = await crypto.subtle.importKey('raw', keyBytes, ED25519, false, ['sign']);
  const sig = await crypto.subtle.sign(ED25519, key, new TextEncoder().encode(registryJson));
  return b64encode(new Uint8Array(sig));
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (request.method === 'GET' && url.pathname === '/nodes') {
      const signed = await env.REGISTRY_KV.get('signed-registry');
      if (!signed) {
        return new Response('{"error":"empty registry"}', {
          status: 200,
          headers: { 'content-type': 'application/json' },
        });
      }
      return new Response(signed, { headers: { 'content-type': 'application/json' } });
    }

    if (request.method === 'POST' && (url.pathname === '/register' || url.pathname === '/revoke')) {
      const token = request.headers.get('x-registry-token') || '';
      if (token !== env.INVITE_TOKEN) {
        return new Response('forbidden', { status: 403 });
      }

      const stored = (await env.REGISTRY_KV.get('registry')) || '{"schema_version":1,"published_at":0,"nodes":[],"revoked":[]}';
      const registry = JSON.parse(stored);
      const body = await request.json();

      if (url.pathname === '/register') {
        registry.nodes.push(body);
      } else {
        registry.revoked.push(body.pubkey);
      }
      registry.published_at = Math.floor(Date.now() / 1000);

      const registryJson = JSON.stringify(registry);
      const signature = await signRegistry(registryJson, env.OPERATOR_KEY_B64);
      const signed = JSON.stringify({ registry: registryJson, signature });

      await env.REGISTRY_KV.put('registry', registryJson);
      await env.REGISTRY_KV.put('signed-registry', signed);
      return new Response('ok', { status: 200 });
    }

    return new Response('not found', { status: 404 });
  },
};
