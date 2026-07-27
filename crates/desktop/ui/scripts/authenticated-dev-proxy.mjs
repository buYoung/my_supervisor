/**
 * Loopback-only UI verification proxy.
 *
 * The browser receives only this proxy origin. The bearer token remains in this
 * process: it is read from the owner-only control-token file immediately before
 * each REST request or WebSocket upgrade and is never returned to the client.
 *
 * Example:
 *   MSV_PROXY_TARGET=http://127.0.0.1:39887 \
 *   MSV_PROXY_TOKEN_FILE=/path/to/run/control.token \
 *   MSV_PROXY_PORT=39999 \
 *   node scripts/authenticated-dev-proxy.mjs
 */
import fs from "node:fs";
import http from "node:http";
import net from "node:net";

const target = new URL(process.env.MSV_PROXY_TARGET ?? "");
const tokenFile = process.env.MSV_PROXY_TOKEN_FILE;
const port = Number(process.env.MSV_PROXY_PORT ?? "");

if (target.protocol !== "http:" || !tokenFile || !Number.isInteger(port) || port < 1) {
  throw new Error("MSV_PROXY_TARGET, MSV_PROXY_TOKEN_FILE, and MSV_PROXY_PORT are required");
}

function authorization() {
  return `Bearer ${fs.readFileSync(tokenFile, "utf8").trim()}`;
}

function forwardedHeaders(headers) {
  const result = { ...headers, host: target.host, authorization: authorization() };
  delete result["proxy-connection"];
  return result;
}

const server = http.createServer((request, response) => {
  const upstream = http.request({
    protocol: target.protocol,
    hostname: target.hostname,
    port: target.port || undefined,
    method: request.method,
    path: request.url,
    headers: forwardedHeaders(request.headers),
  }, (upstreamResponse) => {
    response.writeHead(upstreamResponse.statusCode ?? 502, upstreamResponse.headers);
    upstreamResponse.pipe(response);
  });
  upstream.on("error", () => response.writeHead(502).end());
  request.pipe(upstream);
});

server.on("upgrade", (request, socket, head) => {
  const upstream = net.connect(Number(target.port || 80), target.hostname, () => {
    const headers = forwardedHeaders(request.headers);
    const lines = [`${request.method} ${request.url} HTTP/${request.httpVersion}`];
    for (const [name, value] of Object.entries(headers)) {
      if (value !== undefined) {
        lines.push(`${name}: ${Array.isArray(value) ? value.join(", ") : value}`);
      }
    }
    upstream.write(`${lines.join("\r\n")}\r\n\r\n`);
    if (head.length) upstream.write(head);
    socket.pipe(upstream).pipe(socket);
  });
  upstream.on("error", () => socket.destroy());
});

server.listen(port, "127.0.0.1");
