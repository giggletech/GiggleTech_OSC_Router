const tmi = require('tmi.js');
const osc = require('osc');

const TWITCH_USERNAME = 'your_bot';
const TWITCH_OAUTH = 'oauth:xxxxxxxxxxxxxxxxxxxx';
const TWITCH_CHANNEL = 'your_channel';

// Router target (matches setup.port_rx in config.yml)
const ROUTER_HOST = '127.0.0.1';
const ROUTER_PORT = 9001;
// Use the parameter name from config.yml devices[].proximity_parameter
const PROX_ADDR = '/avatar/parameters/proximity_01';

const udpPort = new osc.UDPPort({
	localAddress: '0.0.0.0',
	localPort: 0,
	metadata: true
});
udpPort.open();

function sendProximity(value) {
	udpPort.send({ address: PROX_ADDR, args: [{ type: 'f', value }] }, ROUTER_HOST, ROUTER_PORT);
}

const client = new tmi.Client({
	identity: { username: TWITCH_USERNAME, password: TWITCH_OAUTH },
	channels: [TWITCH_CHANNEL]
});

client.connect();

client.on('message', (channel, tags, msg) => {
	if (msg.trim().toLowerCase() === '!pat') {
		sendProximity(1.0);
		setTimeout(() => sendProximity(0.0), 1500);
	}
});