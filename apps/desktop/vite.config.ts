import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
    plugins: [vue(), tailwindcss()],
    clearScreen: false,
    server: { port: 5173, strictPort: true },
    build: { target: "es2022", outDir: "dist" },
    define: {
        __APP_VERSION__: JSON.stringify("1.7.3"),
    },
});
