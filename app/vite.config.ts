import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
	plugins: [react()],
	// @noble/curves ships an empty source map that esbuild chokes on.
	optimizeDeps: { esbuildOptions: { sourcemap: false } },
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		host: '127.0.0.1',
		watch: { ignored: ['**/src-tauri/**'] },
	},
});
