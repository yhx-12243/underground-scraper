#!/usr/bin/env node

import { execFile } from 'child_process';
import { request as httpRequest } from 'http';
import { Agent as httpsAgent, get as httpsGet, request as httpsRequest } from 'https';
import { promisify } from 'util';

const execInner = promisify(execFile);

import puppeteer from 'puppeteer';

import Headers from './headers.json' with { type: 'json' };

const sleep = ms => new Promise(f => setTimeout(f, ms));

const UAs = [
	'Mozilla/5.0 (Linux; Android 8.1.0; Moto G (4)) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36 PTST/240201.144844',
	'Mozilla/5.0 (iPhone; CPU iPhone OS 15_3 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/124.0.6261.62 Mobile/15E148 Safari/604.1',
	'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Config/92.2.2788.20',
	'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Agency/98.8.8175.80',
	'Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',
	'Mozilla/5.0 (Linux; Android 11; moto e20 Build/RONS31.267-94-14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36',
	'Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36',
	'Mozilla/5.0 (iPhone; CPU iPhone OS 17_3 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/124.0.6261.62 Mobile/15E148 Safari/604.1',
	'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Trailer/93.3.3516.28',
	'Mozilla/5.0 (Linux; Android 8.1.0; C5 2019 Build/OPM2.171019.012) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36',
	'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',
	'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',

	'Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',
	'Mozilla/5.0 (Linux; Android 6.0; Nexus 5 Build/MRA58N) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36',
	'Mozilla/5.0 (Linux; Android 10; Pixel 4) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36',
	'Mozilla/5.0 (Linux; Android 4.3; Nexus 7 Build/JSS15Q) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',
	'Mozilla/5.0 (iPhone; CPU iPhone OS 13_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/124.0.0.0 Mobile/15E148 Safari/604.1',
	'Mozilla/5.0 (iPad; CPU OS 13_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/124.0.0.0 Mobile/15E148 Safari/604.1',
	'Mozilla/5.0 (X11; CrOS x86_64 10066.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',
	'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_14_6) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36',
];

const credential = {
	username: 'scrapetest',
	password: '9UpIpneGWy8zabx47o',
}, auth = Buffer.from(`${credential.username}:${credential.password}`).toString('base64');

function request(config) {
	let fulfill = () => { };
	const req = (config.protocol.includes('https') ? httpsRequest : httpRequest)(config, res => {
		const buffers = [];
		res.on('data', chunk => buffers.push(chunk));
		res.on('end', () => fulfill([res.headers, Buffer.concat(buffers)]));
	});
	if (config.end) req.end();
	const promise = new Promise((f, reject) => {
		fulfill = f;
		req.on('error', reject);
	});
	promise.req = req;
	return promise;
}

function requestWithProxy(config) {
	// let fulfill = () => {};
	// const req = httpRequest({
	// 	headers: {
	// 		host: `${config.host}:443`,
	// 		'proxy-authorization': `Basic ${auth}`,
	// 	},
	// 	host: config.proxy.host,
	// 	method: 'CONNECT',
	// 	path: `${config.host}:443`,
	// 	port: config.proxy.port,
	// 	protocol: 'http:',
	// });
	// return new Promise((fulfill, reject) => {
	// 	req.on('connect', (res, socket) => {
	// 		if (res.statusCode !== 200) return;
	// 		// const agent = new httpsAgent({ socket });
	// 		const reqInner = httpsGet({
	// 			headers: config.headers,
	// 			path: config.path,
	// 			// host: config.host,
	// 			// socket,
	// 			host: 'localhost',
	// 			port: 4433,
	// 			rejectUnauthorized: false,
	// 		}, res => {
	// 			console.log('response:', res.statusCode, res.statusMessage, config.headers);
	// 			const buffers = [];
	// 			res.on('data', chunk => buffers.push(chunk));
	// 			res.on('end', () => fulfill([res.headers, Buffer.concat(buffers)]));
	// 		});
	// 	});
	// 	req.on('error', reject);
	// 	req.end();
	// });
	return execInner('curl', [
		'--http1.1',
		'-L',
		config.url,
		'--proxy-user', `${credential.username}:${credential.password}`,
		'-x', `http://${config.proxy.host}:${config.proxy.port}`,
		'-H', `Cookie: ${config.headers.Cookie}`,
		'-A', config.headers['User-Agent'],
	]).then(({ stdout }) => stdout);
}

function fetchWork() {
	return request({
		end: true,
		host: 'localhost',
		path: '/get/black',
		port: 18322,
		protocol: 'https:',
		rejectUnauthorized: false,
	}).then(([, body]) => JSON.parse(body));
}

function submit(id, content) {
	const promise = request({
		headers: {
			'content-type': 'application/json'
		},
		host: 'localhost',
		method: 'POST',
		path: '/send/black',
		port: 18322,
		protocol: 'https:',
		rejectUnauthorized: false,
	});
	promise.req.write(JSON.stringify({ id, content }));
	promise.req.end();
	return promise;
}

const port = Number(process.argv[2]);
if (!Number.isSafeInteger(port)) {
	async function worker(port, headers) {
		const nsp = `\x1b[1;35m[${port}]\x1b[0m `;
		for (; ; ) {
			const works = await fetchWork();
			if (!works.length) break;
			for (const work of works) {
				const url = `https://www.blackhatworld.com/seo/${work}`;
				console.log(nsp + '\x1b[33mscraping\x1b[0m %o ...', url);

				const content = await requestWithProxy({
					headers,
					url: `https://www.blackhatworld.com/seo/${work}`,
					proxy: {
						host: 'dc.smartproxy.com',
						port,
					},
				});

				if (!content.match(/<title>.*BlackHatWorld<\/title>/)) {
					console.log(nsp + '\x1b[31mwrong\x1b[0m %o: %s', url, content);
					await sleep(4000);
					continue;
				}

				const [, body2] = await submit(work, content);
				if (body2.toString() !== '""') {
					console.log(nsp + '\x1b[31merror\x1b[0m %o: %s', url, body2.toString());
				} else {
					console.log(nsp + '\x1b[36mfinished\x1b[0m %o ...', url);
				}
				await sleep(2800 + Math.random() * 600);
			}
		}
	}

	const workers = [];
	for (const port_ in Headers) {
		const port = Number(port_);
		if (!Number.isSafeInteger(port)) continue;
		workers.push(worker(port, Headers[port_]));
	}

	await Promise.allSettled(workers);
	process.exit(0);
}

const ua = UAs[Math.floor(Math.random() * UAs.length)];
console.log('choosing user-agent %o ...', ua);

const browser = await puppeteer.launch({
	headless: false,
	args: [`--proxy-server=http://dc.smartproxy.com:${port}`, '--disable-http2'],
});

const [page] = await browser.pages();
await page.setUserAgent(ua);
await page.authenticate(credential);
await page.goto('https://www.blackhatworld.com/');
await page.waitForSelector('div.p-body', { timeout: 0 });

const cookies = await page.cookies();
console.log('cookies:', cookies);
const cookiesDict = cookies.map(cookie => [cookie.name, cookie.value]);
const cookieStr = cookiesDict.map(([name, value]) => `${name}=${value}`).join('; ');

await new Promise(() => {});
