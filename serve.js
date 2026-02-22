const http = require('http');
const fs = require('fs');
const path = require('path');

const https = require('https');
const { WebSocketServer } = require('ws');

const PORT = 8080;

const SECRETS = {
    HXGME_MINT: "8p2K9VoAy6bQgh83M1mFrrsuStw5MtEPnLXkyn7cpump",
    RPC_ENDPOINT: "https://mainnet.helius-rpc.com/?api-key=ee9ffc67-22a1-40e2-aa38-7eef9bccbc61",
    JUP_API_KEY: "2f85db8b-76a2-4077-ba70-5bc196871f44"
};

// MULTIPLAYER STATE
const players = new Map(); // id -> state object

const MIME_TYPES = {
    '.html': 'text/html',
    '.js': 'text/javascript',
    '.css': 'text/css',
    '.wasm': 'application/wasm',
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.json': 'application/json',
    '.glb': 'model/gltf-binary'
};

const server = http.createServer((req, res) => {
    // API Proxy Logic
    if (req.url.startsWith('/api/')) {
        if (req.url === '/api/config') {
            res.writeHead(200, { 'Content-Type': 'application/json' });
            return res.end(JSON.stringify({ HXGME_MINT: SECRETS.HXGME_MINT }));
        }

        if (req.url === '/api/rpc' && req.method === 'POST') {
            let body = '';
            req.on('data', chunk => { body += chunk; });
            req.on('end', () => {
                const proxyReq = https.request(SECRETS.RPC_ENDPOINT, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' }
                }, (proxyRes) => {
                    res.writeHead(proxyRes.statusCode, proxyRes.headers);
                    proxyRes.pipe(res);
                });
                proxyReq.write(body);
                proxyReq.end();
            });
            return;
        }

        if (req.url.startsWith('/api/jupiter/')) {
            const jupPath = req.url.replace('/api/jupiter/', '');
            const jupUrl = `https://api.jup.ag/${jupPath}`;
            
            let body = '';
            req.on('data', chunk => { body += chunk; });
            req.on('end', () => {
                const proxyReq = https.request(jupUrl, {
                    method: req.method,
                    headers: { 
                        'Content-Type': 'application/json',
                        'x-api-key': SECRETS.JUP_API_KEY 
                    }
                }, (proxyRes) => {
                    res.writeHead(proxyRes.statusCode, proxyRes.headers);
                    proxyRes.pipe(res);
                });
                if (body) proxyReq.write(body);
                proxyReq.end();
            });
            return;
        }
    }

    let filePath = req.url === '/' ? './index.html' : '.' + req.url;
    
    // Remove query strings
    filePath = filePath.split('?')[0];
    
    const ext = path.extname(filePath);
    const contentType = MIME_TYPES[ext] || 'application/octet-stream';

    fs.readFile(filePath, (error, content) => {
        if (error) {
            if (error.code === 'ENOENT') {
                res.writeHead(404);
                res.end('File not found');
            } else {
                res.writeHead(500);
                res.end('Internal server error: ' + error.code);
            }
        } else {
            res.writeHead(200, { 
                'Content-Type': contentType,
                'Cache-Control': 'no-cache, no-store, must-revalidate',
                'Pragma': 'no-cache',
                'Expires': '0'
            });
            res.end(content, 'utf-8');
        }
    });
});

const wss = new WebSocketServer({ server });

wss.on('connection', (ws) => {
    let playerId = null;

    ws.on('message', (message, isBinary) => {
        if (isBinary) {
            // High-perf binary relay for move updates
            wss.clients.forEach(client => {
                if (client !== ws && client.readyState === 1) {
                    client.send(message, { binary: true });
                }
            });
            return;
        }

        try {
            const data = JSON.parse(message);
            if (data.type === 'hello') {
                playerId = data.id;
                console.log(`[NET] Player ${playerId} joined`);
            } else if (data.type === 'move') {
                // Fallback JSON move (optional, we'll prefer binary)
                wss.clients.forEach(client => {
                    if (client !== ws && client.readyState === 1) {
                        client.send(message);
                    }
                });
            }
        } catch (e) {
            console.error("[NET] Error parsing message:", e);
        }
    });

    ws.on('close', () => {
        if (playerId) {
            console.log(`[NET] Player ${playerId} left`);
            players.delete(playerId);
            // Notify others
            const msg = JSON.stringify({ type: 'remove', id: playerId });
            wss.clients.forEach(client => {
                if (client !== ws && client.readyState === 1) {
                    client.send(msg);
                }
            });
        }
    });
});

server.listen(PORT, '0.0.0.0', () => {
    console.log(`\n🚀 Game Server running at:`);
    console.log(`   http://localhost:${PORT}`);
    
    // Attempt to show local network IP
    const { networkInterfaces } = require('os');
    const nets = networkInterfaces();
    for (const name of Object.keys(nets)) {
        for (const net of nets[name]) {
            if (net.family === 'IPv4' && !net.internal) {
                console.log(`   http://${net.address}:${PORT} (Mobile Device Link)`);
            }
        }
    }
    console.log('\nKeep this running to serve the game to your iPhone/iPad.');
});
