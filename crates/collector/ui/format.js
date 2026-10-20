// Numbers and times as the page shows them.

export const MiB = 1024 * 1024;

export function fmtAgo(seconds) {
	const s = Math.max(0, Math.round(seconds));
	if (s < 90) return `${s} s ago`;
	if (s < 90 * 60) return `${Math.round(s / 60)} min ago`;
	if (s < 48 * 3600) return `${Math.round(s / 3600)} h ago`;
	return `${Math.round(s / 86400)} days ago`;
}

export function fmtDuration(seconds) {
	const m = Math.floor(seconds / 60), h = Math.floor(m / 60), d = Math.floor(h / 24);
	if (d) return `${d} d ${h % 24} h`;
	if (h) return `${h} h ${m % 60} min`;
	return `${m} min`;
}

export function fmtBytes(b) {
	if (b < 1024) return `${Math.round(b)} B`;
	if (b < MiB) return `${(b / 1024).toFixed(1)} KiB`;
	if (b < 1024 * MiB) return `${(b / MiB).toFixed(1)} MiB`;
	return `${(b / 1024 / MiB).toFixed(1)} GiB`;
}

// Every time on the page: day before month, 24 hours (17/10/2026, 13:29:47),
// whatever the browser's language.
const fmt = (options) => new Intl.DateTimeFormat("en-GB", { hourCycle: "h23", ...options });
const TIME = fmt({ hour: "2-digit", minute: "2-digit" });
export const TIME_S = fmt({ hour: "2-digit", minute: "2-digit", second: "2-digit" });
export const DATE_TIME = fmt({ day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
export const hhmm = (date) => TIME.format(date);

// "today 18:50", "yesterday 18:50", else "17/10 18:50".
export const DAY_MONTH = fmt({ day: "2-digit", month: "2-digit" });
export function whenLabel(date) {
	const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(date).setHours(0, 0, 0, 0)) / 86_400_000);
	const day = days === 0 ? "today" : days === 1 ? "yesterday" : DAY_MONTH.format(date);
	return `${day} ${hhmm(date)}`;
}

// Text for innerHTML: a device's name is typed by someone, or sweeps in from
// the network.
export const esc = (s) => String(s).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
