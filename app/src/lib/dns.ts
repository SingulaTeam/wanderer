// The DNS resolver setting: what the core accepts, checked as it is typed.
// The rules mirror `Resolver::parse` in the tunnel crate, which has the last
// word: a value it refuses is dropped and the default used.

/** The resolvers offered by name; null in the settings is the first. */
export const RESOLVERS = [
	{ id: 'cloudflare', name: 'Cloudflare', value: null },
	{ id: 'google', name: 'Google', value: 'https://8.8.8.8/dns-query' },
	{ id: 'quad9', name: 'Quad9', value: 'https://9.9.9.9/dns-query' },
] as const;

export type ResolverCheck = 'ok' | 'plain' | 'invalid';

/**
 * Whether `typed` is a resolver the core accepts: `https://<IPv4>/<path>`
 * over DNS-over-HTTPS, or a bare IPv4 address over plain DNS (`plain`).
 */
export function checkResolver(typed: string): ResolverCheck {
	const s = typed.trim();
	if (s.startsWith('https://')) {
		const rest = s.slice('https://'.length);
		const slash = rest.indexOf('/');
		if (slash < 0) return 'invalid';
		return isPublicIpv4(rest.slice(0, slash)) && isPath(rest.slice(slash)) ? 'ok' : 'invalid';
	}
	return isPublicIpv4(s) ? 'plain' : 'invalid';
}

/** A public unicast IPv4 address, written as four decimal numbers. */
function isPublicIpv4(s: string): boolean {
	const parts = s.split('.');
	// No leading zeros: the core reads `010` as nothing, not as 8 or 10.
	if (parts.length !== 4 || !parts.every((p) => /^(0|[1-9]\d{0,2})$/.test(p))) return false;
	const [a, b, c, d] = parts.map(Number);
	if ([a, b, c, d].some((n) => n > 255)) return false;
	return !(
		a === 0 ||
		a === 10 ||
		a === 127 ||
		(a === 169 && b === 254) ||
		(a === 172 && b >= 16 && b <= 31) ||
		(a === 192 && b === 168) ||
		a >= 224
	);
}

/** An HTTP path with no query, fragment or escapes. */
function isPath(s: string): boolean {
	return s.startsWith('/') && s.length <= 256 && /^[\x21-\x7e]*$/.test(s) && !/[?#%]/.test(s);
}
