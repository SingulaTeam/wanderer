import type { Config } from 'tailwindcss';

// Wanderer's palette, from the landing's dark theme. Dark only, by choice.
export default {
	content: ['./index.html', './src/**/*.{ts,tsx}'],
	theme: {
		extend: {
			colors: {
				bg: '#0b0b0d',
				card: '#141417',
				raised: '#1f1f24',
				line: 'rgba(255,255,255,0.08)',
				fg: '#f4f4f5',
				muted: '#a1a1aa',
				brand: { DEFAULT: '#fe5001', soft: '#ffb48f', hover: '#ff7a3d' },
				hold: { DEFAULT: '#fbbf24', soft: '#fcd34d' },
				ok: '#4ade80',
			},
			fontFamily: {
				sans: ['Inter', 'system-ui', 'sans-serif'],
				display: ['Orbitron', 'Inter', 'sans-serif'],
			},
			keyframes: {
				orbit: { to: { transform: 'rotate(360deg)' } },
				breathe: { '0%, 100%': { opacity: '0.55' }, '50%': { opacity: '1' } },
				rise: { from: { transform: 'translateY(100%)' }, to: { transform: 'translateY(0)' } },
				fade: { from: { opacity: '0' }, to: { opacity: '1' } },
			},
			animation: {
				orbit: 'orbit 14s linear infinite',
				'orbit-fast': 'orbit 2.4s linear infinite',
				breathe: 'breathe 2.2s ease-in-out infinite',
				rise: 'rise 0.28s cubic-bezier(0.22, 1, 0.36, 1)',
				fade: 'fade 0.2s ease-out',
			},
		},
	},
	plugins: [],
} satisfies Config;
