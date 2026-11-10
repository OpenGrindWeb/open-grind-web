// Open Grind web bridge.
//
// og-server injects this classic script at the top of index.html, before the
// SvelteKit bundle. It provides the globals that @tauri-apps/api reads
// (`__TAURI_INTERNALS__`, `__TAURI_OS_PLUGIN_INTERNALS__`,
// `__TAURI_EVENT_PLUGIN_INTERNALS__`, `isTauri`) so the upstream front runs
// unmodified, as if inside a Tauri iOS webview:
//
// - native-only commands (geolocation, clipboard, haptics, updater...) are
//   answered here with web APIs or harmless defaults;
// - Tauri events are delivered from the server's /events WebSocket and from
//   window focus changes;
// - every other command goes to the server as POST /ipc/<command>.
(() => {
	"use strict";

	const callbacks = new Map();
	let nextCallbackId = 1;
	// event name -> Map<eventId, callbackId>
	const listeners = new Map();
	let nextEventId = 1;

	function transformCallback(callback, once = false) {
		const id = nextCallbackId++;
		callbacks.set(id, (...args) => {
			if (once) callbacks.delete(id);
			return callback && callback(...args);
		});
		return id;
	}

	function unregisterCallback(id) {
		callbacks.delete(id);
	}

	function dispatch(event, payload) {
		const forEvent = listeners.get(event);
		if (!forEvent) return;
		for (const [eventId, callbackId] of [...forEvent]) {
			try {
				callbacks.get(callbackId)?.({ event, id: eventId, payload });
			} catch (error) {
				console.error(`[og-bridge] listener for ${event} threw`, error);
			}
		}
	}

	// --- server events -----------------------------------------------------

	let socketDelay = 500;
	function connectEvents() {
		const scheme = location.protocol === "https:" ? "wss:" : "ws:";
		const socket = new WebSocket(`${scheme}//${location.host}/events`);
		socket.onopen = () => {
			socketDelay = 500;
		};
		socket.onmessage = (message) => {
			let parsed;
			try {
				parsed = JSON.parse(message.data);
			} catch {
				return;
			}
			if (parsed && typeof parsed.event === "string") {
				dispatch(parsed.event, parsed.payload);
			}
		};
		socket.onclose = () => {
			setTimeout(connectEvents, socketDelay);
			socketDelay = Math.min(socketDelay * 2, 15000);
		};
	}
	connectEvents();

	addEventListener("focus", () => dispatch("tauri://focus", null));
	addEventListener("blur", () => dispatch("tauri://blur", null));

	// --- local command implementations --------------------------------------

	const unsupportedUpdate = {
		state: "unsupported",
		detail: { reason: "noReleaseArtifacts", detail: { target: "web" } },
	};

	function geolocationState(state) {
		return { location: state, coarseLocation: state };
	}

	async function permissionState() {
		try {
			const status = await navigator.permissions.query({ name: "geolocation" });
			return status.state;
		} catch {
			return "prompt";
		}
	}

	function currentPosition(options = {}) {
		return new Promise((resolve, reject) => {
			if (!("geolocation" in navigator)) {
				reject("Geolocation is not available");
				return;
			}
			navigator.geolocation.getCurrentPosition(
				({ coords, timestamp }) =>
					resolve({
						timestamp,
						coords: {
							latitude: coords.latitude,
							longitude: coords.longitude,
							accuracy: coords.accuracy,
							altitude: coords.altitude,
							altitudeAccuracy: coords.altitudeAccuracy,
							speed: coords.speed,
							heading: coords.heading,
						},
					}),
				(error) => reject(error.message || "Location error"),
				{
					enableHighAccuracy: options.enableHighAccuracy ?? true,
					// The native plugin treats 0 as "no timeout"; the web API
					// would fail immediately.
					timeout: options.timeout > 0 ? options.timeout : 30000,
					maximumAge: options.maximumAge ?? 0,
				},
			);
		});
	}

	const local = {
		"plugin:event|listen": ({ event, handler }) => {
			const eventId = nextEventId++;
			if (!listeners.has(event)) listeners.set(event, new Map());
			listeners.get(event).set(eventId, handler);
			return eventId;
		},
		"plugin:event|unlisten": ({ event, eventId }) => {
			listeners.get(event)?.delete(eventId);
			return null;
		},
		"plugin:event|emit": ({ event, payload }) => {
			dispatch(event, payload);
			return null;
		},
		"plugin:event|emit_to": ({ event, payload }) => {
			dispatch(event, payload);
			return null;
		},

		"plugin:geolocation|check_permissions": async () =>
			geolocationState(await permissionState()),
		"plugin:geolocation|request_permissions": async () => {
			try {
				await currentPosition({ enableHighAccuracy: false, maximumAge: 600000 });
				return geolocationState("granted");
			} catch {
				return geolocationState(await permissionState());
			}
		},
		"plugin:geolocation|get_current_position": ({ options } = {}) =>
			currentPosition(options),

		"plugin:clipboard-manager|write_text": async ({ text } = {}) => {
			await navigator.clipboard.writeText(text ?? "");
			return null;
		},
		"plugin:clipboard-manager|read_text": () => navigator.clipboard.readText(),

		"plugin:opener|open_url": ({ url } = {}) => {
			window.open(url, "_blank", "noopener,noreferrer");
			return null;
		},

		"plugin:app|version": () => "0.1.0-web",
		"plugin:app|name": () => "Open Grind",
		"plugin:app|tauri_version": () => "2.0.0",

		backdrop_filter_renders: () => true,
		webview_engine: () => null,
		set_scroll_gesture_capture: () => null,
		play_haptic: () => null,
		open_app_settings: () => null,
		desktop_entry_state: () => ({ available: false, installed: false }),

		updater_capability: () => unsupportedUpdate,
		updater_readiness: () => unsupportedUpdate,
		updater_progress: () => null,
		updater_install_pending: () => false,
		updater_installed_version: () => null,
	};

	// --- invoke --------------------------------------------------------------

	async function remote(cmd, args) {
		let response;
		try {
			response = await fetch(`/ipc/${encodeURIComponent(cmd)}`, {
				method: "POST",
				credentials: "same-origin",
				headers: { "content-type": "application/json", "x-og-ipc": "1" },
				body: JSON.stringify(args ?? {}),
			});
		} catch (error) {
			throw { kind: "Connect", message: String(error) };
		}
		if (response.status === 401) {
			location.assign("/gate");
			throw { kind: "NotSignedIn" };
		}
		const body = await response.json().catch(() => null);
		if (!response.ok) {
			throw body ?? { kind: "Http", message: `HTTP ${response.status}` };
		}
		return body;
	}

	async function invoke(cmd, args = {}, _options) {
		if (Object.hasOwn(local, cmd)) return local[cmd](args);
		if (cmd.startsWith("plugin:window|") || cmd.startsWith("plugin:webview|")) {
			return null;
		}
		if (cmd.startsWith("plugin:")) {
			throw `${cmd} is not available in the web build`;
		}
		return remote(cmd, args);
	}

	const iosVersion = /OS (\d+)[._](\d+)/.exec(navigator.userAgent);

	Object.assign(window, {
		isTauri: true,
		__OG_WEB__: true,
		__TAURI_OS_PLUGIN_INTERNALS__: {
			eol: "\n",
			platform: "ios",
			os_type: "ios",
			family: "unix",
			arch: "aarch64",
			version: iosVersion ? `${iosVersion[1]}.${iosVersion[2]}` : "18.0",
			exe_extension: "",
		},
		__TAURI_EVENT_PLUGIN_INTERNALS__: {
			unregisterListener(event, eventId) {
				listeners.get(event)?.delete(eventId);
			},
		},
		__TAURI_INTERNALS__: {
			invoke,
			transformCallback,
			unregisterCallback,
			runCallback(id, data) {
				callbacks.get(id)?.(data);
			},
			callbacks,
			convertFileSrc(filePath, protocol = "asset") {
				return `/media/${encodeURIComponent(protocol)}/${encodeURIComponent(filePath)}`;
			},
			metadata: {
				currentWindow: { label: "main" },
				currentWebview: { windowLabel: "main", label: "main" },
			},
			plugins: {},
		},
	});
})();
