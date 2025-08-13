const tmi = require('tmi.js');
const osc = require('osc');
try { require('dotenv').config(); } catch (_) {}

// Configuration via environment variables (see .env.example)
const TWITCH_USERNAME = process.env.TWITCH_USERNAME || 'your_bot';
const TWITCH_CHANNEL = process.env.TWITCH_CHANNEL || 'your_channel';

const TWITCH_CLIENT_ID = process.env.TWITCH_CLIENT_ID || '';
const TWITCH_CLIENT_SECRET = process.env.TWITCH_CLIENT_SECRET || '';

// Supabase REST access (server-side only; use service role key)
const SUPABASE_URL = process.env.SUPABASE_URL || '';
const SUPABASE_SERVICE_ROLE_KEY = process.env.SUPABASE_SERVICE_ROLE_KEY || '';
const SUPABASE_TABLE = process.env.SUPABASE_TABLE || 'twitch_tokens';

// Router target (matches setup.port_rx in config.yml)
const ROUTER_HOST = process.env.ROUTER_HOST || '127.0.0.1';
const ROUTER_PORT = Number(process.env.ROUTER_PORT || 9001);
// Use the parameter name from config.yml devices[].proximity_parameter
const PROX_ADDR = process.env.PROX_ADDR || '/avatar/parameters/proximity_01';

const udpPort = new osc.UDPPort({
	localAddress: '0.0.0.0',
	localPort: 0,
	metadata: true
});
udpPort.open();

function sendProximity(value) {
	udpPort.send({ address: PROX_ADDR, args: [{ type: 'f', value }] }, ROUTER_HOST, ROUTER_PORT);
}

async function supabaseRequest(path, init) {
	if (!SUPABASE_URL || !SUPABASE_SERVICE_ROLE_KEY) {
		throw new Error('SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY are required');
	}
	const url = `${SUPABASE_URL.replace(/\/$/, '')}/rest/v1${path}`;
	const headers = Object.assign({
		'apikey': SUPABASE_SERVICE_ROLE_KEY,
		'Authorization': `Bearer ${SUPABASE_SERVICE_ROLE_KEY}`,
		'Content-Type': 'application/json'
	}, init && init.headers ? init.headers : {});
	const res = await fetch(url, Object.assign({}, init, { headers }));
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`Supabase request failed: ${res.status} ${res.statusText} - ${text}`);
	}
	// Some requests may return empty
	const contentType = res.headers.get('content-type') || '';
	if (!contentType.includes('application/json')) return null;
	return res.json();
}

async function getChannelTokens(channel) {
	const rows = await supabaseRequest(`/${encodeURIComponent(SUPABASE_TABLE)}?channel=eq.${encodeURIComponent(channel)}&select=access_token,refresh_token,expires_at`, {
		method: 'GET'
	});
	return Array.isArray(rows) && rows.length > 0 ? rows[0] : null;
}

async function upsertChannelTokens(channel, tokens) {
	const body = [{
		channel,
		access_token: tokens.access_token,
		refresh_token: tokens.refresh_token,
		expires_at: tokens.expires_at
	}];
	await supabaseRequest(`/${encodeURIComponent(SUPABASE_TABLE)}`, {
		method: 'POST',
		headers: { 'Prefer': 'resolution=merge-duplicates' },
		body: JSON.stringify(body)
	});
}

function isExpiredOrNear(expiryIso, skewSeconds = 60) {
	if (!expiryIso) return true;
	const expiresAtMs = new Date(expiryIso).getTime();
	return Date.now() >= (expiresAtMs - skewSeconds * 1000);
}

async function refreshTwitchToken(refreshToken) {
	if (!TWITCH_CLIENT_ID || !TWITCH_CLIENT_SECRET) {
		throw new Error('TWITCH_CLIENT_ID and TWITCH_CLIENT_SECRET are required to refresh tokens');
	}
	const params = new URLSearchParams();
	params.set('grant_type', 'refresh_token');
	params.set('refresh_token', refreshToken);
	params.set('client_id', TWITCH_CLIENT_ID);
	params.set('client_secret', TWITCH_CLIENT_SECRET);
	const res = await fetch('https://id.twitch.tv/oauth2/token', {
		method: 'POST',
		headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
		body: params.toString()
	});
	if (!res.ok) {
		const text = await res.text();
		throw new Error(`Failed to refresh Twitch token: ${res.status} ${res.statusText} - ${text}`);
	}
	const data = await res.json();
	const expiresAt = new Date(Date.now() + (Math.max(0, (data.expires_in || 0) - 60)) * 1000).toISOString();
	return { access_token: data.access_token, refresh_token: data.refresh_token || refreshToken, expires_at: expiresAt };
}

async function ensureAccessToken(channel) {
	const row = await getChannelTokens(channel);
	if (!row) throw new Error(`No tokens found in Supabase for channel "${channel}"`);
	if (!isExpiredOrNear(row.expires_at)) return row.access_token;
	const refreshed = await refreshTwitchToken(row.refresh_token);
	await upsertChannelTokens(channel, refreshed);
	return refreshed.access_token;
}

async function start() {
	try {
		const accessToken = await ensureAccessToken(TWITCH_CHANNEL);
		const client = new tmi.Client({
			identity: { username: TWITCH_USERNAME, password: `oauth:${accessToken}` },
			channels: [TWITCH_CHANNEL]
		});
		await client.connect();
		client.on('message', (channel, tags, msg) => {
			if (msg.trim().toLowerCase() === '!pat') {
				sendProximity(1.0);
				setTimeout(() => sendProximity(0.0), 1500);
			}
		});
		console.log(`Connected to Twitch chat as ${TWITCH_USERNAME} in #${TWITCH_CHANNEL}`);
	} catch (err) {
		console.error(err);
		process.exitCode = 1;
	}
}

// Node 18+ has global fetch. If not available, require('node-fetch') and assign to globalThis.fetch
if (typeof fetch !== 'function') {
	try {
		globalThis.fetch = require('node-fetch');
	} catch (_) {
		throw new Error('fetch is not available. Use Node 18+ or install node-fetch');
	}
}

start();